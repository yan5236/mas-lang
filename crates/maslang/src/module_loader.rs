use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{
    AssignTarget, ConditionalBranch, Expr, ImportItem, ImportKind, ImportPath, MapEntry, Parameter,
    Program, Stmt, TypeName,
};
use crate::diagnostic::CompilerError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

/// 保存模块装载后的扁平化源码与最终程序，供后续语义与代码生成复用。
pub struct ModuleLoadResult {
    pub origins: Vec<crate::source_map::ModuleOrigin>,
    pub source: SourceFile,
    pub program: Program,
    pub package_roots: Vec<PathBuf>,
}

/// 从入口源码开始装载整个模块图，并把模块重写为单个可编译程序。
pub fn load_module_graph(
    entry: &SourceFile,
    settings: &CompilerSettings,
) -> Result<ModuleLoadResult, CompilerError> {
    let Some(entry_path) = canonicalize_existing_path(Path::new(entry.path())) else {
        return parse_single_source(entry, settings);
    };
    let Some(project_root) = discover_project_root(&entry_path) else {
        return parse_single_source(entry, settings);
    };
    let entry_package_root = discover_package_root(&entry_path, &project_root)?;
    let mut loader = ModuleLoader::new(settings, project_root);
    loader.load_module_with_source(&entry_path, &entry_package_root, true, Some(entry.clone()))?;
    Ok(ModuleLoadResult {
        origins: loader.origins,
        source: SourceFile::new(entry.path(), loader.synthetic_source),
        program: Program {
            statements: loader.statements,
        },
        package_roots: loader.package_roots.into_iter().collect(),
    })
}

/// 维护模块装载过程中的缓存、循环检测和最终扁平化结果。
struct ModuleLoader<'a> {
    settings: &'a CompilerSettings,
    project_root: PathBuf,
    loaded_modules: BTreeMap<PathBuf, LoadedModule>,
    active_modules: HashSet<PathBuf>,
    dependency_cache: BTreeMap<PathBuf, BTreeMap<String, DependencyVersion>>,
    package_roots: BTreeSet<PathBuf>,
    origins: Vec<crate::source_map::ModuleOrigin>,
    synthetic_source: String,
    statements: Vec<Stmt>,
}

/// 保存项目设置中模块解析所需的依赖版本。
#[derive(Clone, serde::Deserialize)]
struct DependencyVersion {
    version: String,
}

/// 保存单个模块对外可见的公开符号映射。
#[derive(Clone)]
struct LoadedModule {
    exports: BTreeMap<String, String>,
}

/// 基于模块作用域重写标识符，把导入名和顶层声明名都替换成内部稳定名。
struct ModuleRewriter {
    bindings: BTreeMap<String, String>,
    local_scopes: Vec<HashSet<String>>,
}

impl<'a> ModuleLoader<'a> {
    /// 为一次模块装载流程创建新的装载器实例。
    fn new(settings: &'a CompilerSettings, project_root: PathBuf) -> Self {
        Self {
            settings,
            project_root,
            loaded_modules: BTreeMap::new(),
            active_modules: HashSet::new(),
            dependency_cache: BTreeMap::new(),
            package_roots: BTreeSet::new(),
            origins: Vec::new(),
            synthetic_source: String::new(),
            statements: Vec::new(),
        }
    }

    /// 装载单个模块文件，递归处理其导入关系并缓存公开符号表。
    fn load_module(
        &mut self,
        module_path: &Path,
        package_root: &Path,
        is_entry: bool,
    ) -> Result<LoadedModule, CompilerError> {
        self.load_module_with_source(module_path, package_root, is_entry, None)
    }

    /// 装载模块时允许调用方为入口模块提供内存源码，确保编辑器未保存文本也能参与分析。
    fn load_module_with_source(
        &mut self,
        module_path: &Path,
        package_root: &Path,
        is_entry: bool,
        source_override: Option<SourceFile>,
    ) -> Result<LoadedModule, CompilerError> {
        if let Some(loaded) = self.loaded_modules.get(module_path) {
            return Ok(loaded.clone());
        }

        self.package_roots.insert(package_root.to_path_buf());

        if !self.active_modules.insert(module_path.to_path_buf()) {
            return Err(CompilerError::build_message(
                format!("模块图中检测到循环依赖：{}", module_path.display()),
                Span::new(0, 0),
            ));
        }

        let source = match source_override {
            Some(source) => source,
            None => read_module_source(module_path)?,
        };
        let tokens = Lexer::new(&source, self.settings)
            .tokenize()
            .map_err(|error| error.with_source(source.clone()))?;
        let program = Parser::new(tokens, self.settings)
            .parse_program()
            .map_err(|error| error.with_source(source.clone()))?;
        let renamed_declarations = collect_top_level_declarations(&program, module_path, is_entry);
        let mut imports = BTreeMap::new();
        let mut exports = BTreeMap::new();
        let mut module_aliases = BTreeMap::new();

        for statement in &program.statements {
            match statement {
                Stmt::Import { path, kind, span } => {
                    if let Some(alias) = &path.alias
                        && module_aliases.insert(alias, *span).is_some()
                    {
                        return Err(CompilerError::build_message(
                            format!("模块别名 `{alias}` 重复"),
                            *span,
                        ));
                    }
                    self.resolve_import_bindings(package_root, path, kind, &mut imports)?;
                }
                Stmt::ExportImport { path, kind, .. } => self.resolve_export_bindings(
                    package_root,
                    path,
                    kind,
                    &mut imports,
                    &mut exports,
                )?,
                Stmt::StructDecl {
                    is_public, name, ..
                }
                | Stmt::FunctionDecl {
                    is_public, name, ..
                }
                | Stmt::ExternFunctionDecl {
                    is_public, name, ..
                }
                | Stmt::VarDecl {
                    is_public, name, ..
                } => {
                    if *is_public {
                        let internal = renamed_declarations.get(name).ok_or_else(|| {
                            CompilerError::build_message(
                                format!("公开符号 `{name}` 缺少内部重写目标"),
                                statement.span(),
                            )
                        })?;
                        exports.insert(name.clone(), internal.clone());
                    }
                }
                _ => {}
            }
        }

        for (alias, span) in module_aliases {
            if imports.contains_key(alias) || renamed_declarations.contains_key(alias) {
                return Err(CompilerError::build_message(
                    format!("模块别名 `{alias}` 与当前模块的导入名或顶层声明重名"),
                    span,
                ));
            }
        }

        for name in imports.keys() {
            if renamed_declarations.contains_key(name) {
                return Err(CompilerError::build_message(
                    format!("导入名 `{name}` 与当前模块顶层声明重名"),
                    Span::new(0, 0),
                ));
            }
        }

        let mut bindings = renamed_declarations;
        bindings.extend(imports);
        let mut rewriter = ModuleRewriter::new(bindings.clone());
        let rewritten_statements = rewriter.rewrite_program(program);
        let offset = append_module_source(&mut self.synthetic_source, module_path, &source);
        self.origins.push(crate::source_map::ModuleOrigin {
            offset,
            source,
            bindings,
        });
        self.statements.extend(
            rewritten_statements
                .into_iter()
                .filter(|statement| {
                    !matches!(statement, Stmt::Import { .. } | Stmt::ExportImport { .. })
                })
                .map(|statement| shift_stmt(statement, offset)),
        );

        let loaded = LoadedModule { exports };
        self.loaded_modules
            .insert(module_path.to_path_buf(), loaded.clone());
        self.active_modules.remove(module_path);
        Ok(loaded)
    }

    /// 加载普通导入的目标模块，并将选中的公开成员写入当前模块的绑定表。
    fn resolve_import_bindings(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
        kind: &ImportKind,
        imports: &mut BTreeMap<String, String>,
    ) -> Result<(), CompilerError> {
        let (target_file, target_package_root) = self.resolve_module_path(package_root, path)?;
        let target_module = self.load_module(&target_file, &target_package_root, false)?;
        merge_imports(kind, &target_module.exports, imports, path)?;
        Ok(())
    }

    /// 解析 `export import` 语句，并同时更新导入绑定和公开导出表。
    fn resolve_export_bindings(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
        kind: &ImportKind,
        imports: &mut BTreeMap<String, String>,
        exports: &mut BTreeMap<String, String>,
    ) -> Result<(), CompilerError> {
        let (target_file, target_package_root) = self.resolve_module_path(package_root, path)?;
        let target_module = self.load_module(&target_file, &target_package_root, false)?;
        let merged = merge_imports(kind, &target_module.exports, imports, path)?;
        exports.extend(merged);
        Ok(())
    }

    /// 根据导入路径决定应解析为当前包本地模块，还是项目根目录中的已安装依赖模块。
    fn resolve_module_path(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
    ) -> Result<(PathBuf, PathBuf), CompilerError> {
        let segments = path.normalized_segments();
        if segments.is_empty() {
            return Err(CompilerError::syntax_message("导入路径不能为空", path.span));
        }

        if !path.is_local() {
            let dependencies = self.read_dependencies(package_root)?;
            if let Some(dependency) = dependencies.get(&segments[0]).cloned() {
                let dependency_root =
                    self.find_dependency_root(&segments[0], &dependency.version, path.span)?;
                let target =
                    resolve_module_file(&dependency_root.join("src"), &segments[1..], path.span)?;
                return Ok((target, dependency_root));
            }
        }

        let target = resolve_module_file(&package_root.join("src"), segments, path.span)?;
        Ok((target, package_root.to_path_buf()))
    }

    /// 读取并缓存当前包的 TOML 依赖表；清单格式错误时返回带路径的诊断。
    fn read_dependencies(
        &mut self,
        package_root: &Path,
    ) -> Result<&BTreeMap<String, DependencyVersion>, CompilerError> {
        #[derive(serde::Deserialize)]
        struct Settings {
            #[serde(default)]
            dependencies: BTreeMap<String, DependencyVersion>,
        }

        let manifest_path = package_root.join("settings.toml");
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.dependency_cache.entry(package_root.to_path_buf())
        {
            let text = fs::read_to_string(&manifest_path).map_err(|error| {
                CompilerError::build_message(
                    format!("读取项目设置失败：{}: {error}", manifest_path.display()),
                    Span::new(0, 0),
                )
            })?;
            let settings: Settings = toml::from_str(&text).map_err(|error| {
                CompilerError::build_message(
                    format!("解析项目设置失败：{}: {error}", manifest_path.display()),
                    Span::new(0, 0),
                )
            })?;
            entry.insert(settings.dependencies);
        }
        Ok(&self.dependency_cache[package_root])
    }

    /// 按清单中的精确版本定位已安装依赖，避免历史缓存参与选择。
    fn find_dependency_root(
        &self,
        name: &str,
        version: &str,
        span: Span,
    ) -> Result<PathBuf, CompilerError> {
        if !crate::settings::is_dependency_cache_component(name)
            || !crate::settings::is_dependency_cache_component(version)
        {
            return Err(CompilerError::build_message(
                "依赖名称或版本含有不安全的缓存路径字符",
                span,
            ));
        }
        let path = self
            .project_root
            .join("mod_module")
            .join(format!("{name}@{version}"));
        if path.is_dir() {
            return Ok(path);
        }
        Err(CompilerError::build_message(
            format!("未安装依赖 `{name}@{version}`，请先执行 fama install"),
            span,
        ))
    }
}

impl ModuleRewriter {
    /// 使用当前模块可见绑定表创建新的符号重写器。
    fn new(bindings: BTreeMap<String, String>) -> Self {
        Self {
            bindings,
            local_scopes: vec![HashSet::new()],
        }
    }

    /// 重写整个模块程序，统一替换导入名和顶层声明名。
    fn rewrite_program(&mut self, program: Program) -> Vec<Stmt> {
        program
            .statements
            .into_iter()
            .map(|statement| self.rewrite_stmt(statement, true))
            .collect()
    }

    /// 按作用域重写单条语句中的声明名和引用名。
    fn rewrite_stmt(&mut self, statement: Stmt, is_top_level: bool) -> Stmt {
        match statement {
            Stmt::StructDecl {
                is_public,
                name,
                fields,
                span,
            } => Stmt::StructDecl {
                is_public,
                name: self.rename_name(&name),
                fields: self.rewrite_parameters(fields),
                span,
            },
            Stmt::FunctionDecl {
                is_public: _,
                name,
                parameters,
                return_type,
                body,
                span,
            } => {
                let new_name = if is_top_level {
                    self.rename_name(&name)
                } else {
                    name.clone()
                };
                self.local_scopes
                    .push(parameters.iter().map(|item| item.name.clone()).collect());
                let body = body
                    .into_iter()
                    .map(|nested| self.rewrite_stmt(nested, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::FunctionDecl {
                    is_public: false,
                    name: new_name,
                    parameters: self.rewrite_parameters(parameters),
                    return_type: return_type.map(|t| self.rewrite_type(t)),
                    body,
                    span,
                }
            }
            Stmt::ExternFunctionDecl {
                is_public: _,
                name,
                parameters,
                return_type,
                span,
            } => Stmt::ExternFunctionDecl {
                is_public: false,
                name: if is_top_level {
                    self.rename_name(&name)
                } else {
                    name
                },
                parameters: self.rewrite_parameters(parameters),
                return_type: return_type.map(|t| self.rewrite_type(t)),
                span,
            },
            Stmt::VarDecl {
                is_public: _,
                binding_kind,
                name,
                annotation,
                initializer,
                span,
            } => {
                let initializer = initializer.map(|expr| self.rewrite_expr(expr));
                if !is_top_level {
                    self.current_scope_mut().insert(name.clone());
                }
                Stmt::VarDecl {
                    is_public: false,
                    binding_kind,
                    name: if is_top_level {
                        self.rename_name(&name)
                    } else {
                        name
                    },
                    annotation: annotation.map(|t| self.rewrite_type(t)),
                    initializer,
                    span,
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => Stmt::Assign {
                target: self.rewrite_target(target),
                op,
                value: self.rewrite_expr(value),
                span,
            },
            Stmt::If {
                branches,
                else_branch,
                span,
            } => Stmt::If {
                branches: branches
                    .into_iter()
                    .map(|branch| ConditionalBranch {
                        condition: self.rewrite_expr(branch.condition),
                        body: self.rewrite_block(branch.body),
                        span: branch.span,
                    })
                    .collect(),
                else_branch: else_branch.map(|body| self.rewrite_block(body)),
                span,
            },
            Stmt::While {
                condition,
                body,
                span,
            } => Stmt::While {
                condition: self.rewrite_expr(condition),
                body: self.rewrite_block(body),
                span,
            },
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => {
                self.local_scopes.push(HashSet::from([name.clone()]));
                let body = body
                    .into_iter()
                    .map(|item| self.rewrite_stmt(item, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::ForRange {
                    name,
                    start: self.rewrite_expr(start),
                    end: self.rewrite_expr(end),
                    body,
                    span,
                }
            }
            Stmt::ForEach {
                key_name,
                value_name,
                iterable,
                body,
                span,
            } => {
                let mut scope = HashSet::from([key_name.clone()]);
                if let Some(value_name) = &value_name {
                    scope.insert(value_name.clone());
                }
                self.local_scopes.push(scope);
                let body = body
                    .into_iter()
                    .map(|item| self.rewrite_stmt(item, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::ForEach {
                    key_name,
                    value_name,
                    iterable: self.rewrite_expr(iterable),
                    body,
                    span,
                }
            }
            Stmt::Return { value, span } => Stmt::Return {
                value: value.map(|expr| self.rewrite_expr(expr)),
                span,
            },
            Stmt::ExprStmt { expr, span } => Stmt::ExprStmt {
                expr: self.rewrite_expr(expr),
                span,
            },
            other => other,
        }
    }

    /// 为嵌套代码块创建独立的局部作用域，避免错误替换局部变量名。
    fn rewrite_block(&mut self, body: Vec<Stmt>) -> Vec<Stmt> {
        self.local_scopes.push(HashSet::new());
        let rewritten = body
            .into_iter()
            .map(|item| self.rewrite_stmt(item, false))
            .collect();
        self.local_scopes.pop();
        rewritten
    }

    /// 按作用域重写赋值目标；模块成员赋值交由具名绑定的可变性检查处理。
    fn rewrite_target(&mut self, target: AssignTarget) -> AssignTarget {
        match target {
            AssignTarget::Field { target, name, span } => {
                if let Some(name) = self.qualified_binding(&target, &name) {
                    return AssignTarget::Identifier { name, span };
                }
                AssignTarget::Field {
                    target: Box::new(self.rewrite_expr(*target)),
                    name,
                    span,
                }
            }
            AssignTarget::Identifier { name, span } => AssignTarget::Identifier {
                name: self.rewrite_identifier(name),
                span,
            },
            AssignTarget::Index {
                target,
                index,
                span,
            } => AssignTarget::Index {
                target: Box::new(self.rewrite_expr(*target)),
                index: Box::new(self.rewrite_expr(*index)),
                span,
            },
        }
    }

    /// 递归重写标识符和模块成员；成员名即使与内建方法或属性同名，也优先使用模块绑定。
    fn rewrite_expr(&mut self, expr: Expr) -> Expr {
        match expr {
            Expr::FieldAccess { target, name, span } => {
                if let Some(name) = self.qualified_binding(&target, &name) {
                    return Expr::Identifier { name, span };
                }
                Expr::FieldAccess {
                    target: Box::new(self.rewrite_expr(*target)),
                    name,
                    span,
                }
            }
            Expr::Identifier { name, span } => Expr::Identifier {
                name: self.rewrite_identifier(name),
                span,
            },
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => Expr::Binary {
                left: Box::new(self.rewrite_expr(*left)),
                op,
                right: Box::new(self.rewrite_expr(*right)),
                span,
            },
            Expr::Unary { op, operand, span } => Expr::Unary {
                op,
                operand: Box::new(self.rewrite_expr(*operand)),
                span,
            },
            Expr::Call {
                callee,
                arguments,
                span,
            } => Expr::Call {
                callee: Box::new(self.rewrite_expr(*callee)),
                arguments: arguments
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::ListLiteral { elements, span } => Expr::ListLiteral {
                elements: elements
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::MapLiteral { entries, span } => Expr::MapLiteral {
                entries: entries
                    .into_iter()
                    .map(|entry| MapEntry {
                        key: self.rewrite_expr(entry.key),
                        value: self.rewrite_expr(entry.value),
                        span: entry.span,
                    })
                    .collect(),
                span,
            },
            Expr::ArrayInitializer {
                element_type,
                size,
                elements,
                span,
            } => Expr::ArrayInitializer {
                element_type,
                size,
                elements: elements
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::Index {
                target,
                index,
                span,
            } => Expr::Index {
                target: Box::new(self.rewrite_expr(*target)),
                index: Box::new(self.rewrite_expr(*index)),
                span,
            },
            Expr::Slice {
                target,
                start,
                end,
                span,
            } => Expr::Slice {
                target: Box::new(self.rewrite_expr(*target)),
                start: start.map(|expr| Box::new(self.rewrite_expr(*expr))),
                end: end.map(|expr| Box::new(self.rewrite_expr(*expr))),
                span,
            },
            Expr::MethodCall {
                target,
                method,
                arguments,
                span,
            } => {
                let arguments = arguments
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect();
                if let Some(name) = self.qualified_binding(&target, method.display_name()) {
                    return Expr::Call {
                        callee: Box::new(Expr::Identifier { name, span }),
                        arguments,
                        span,
                    };
                }
                Expr::MethodCall {
                    target: Box::new(self.rewrite_expr(*target)),
                    method,
                    arguments,
                    span,
                }
            }
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => {
                if let Some(name) = self.qualified_binding(&target, property.display_name()) {
                    return Expr::Identifier { name, span };
                }
                Expr::PropertyAccess {
                    target: Box::new(self.rewrite_expr(*target)),
                    property,
                    span,
                }
            }
            Expr::Grouped { expression, span } => Expr::Grouped {
                expression: Box::new(self.rewrite_expr(*expression)),
                span,
            },
            other => other,
        }
    }

    /// 查找 `模块别名.成员` 的内部名称；局部变量同名时保留普通的对象成员访问。
    fn qualified_binding(&self, target: &Expr, member: &str) -> Option<String> {
        let Expr::Identifier { name, .. } = target else {
            return None;
        };
        if self.is_local_name(name) {
            return None;
        }
        self.bindings.get(&format!("{name}.{member}")).cloned()
    }

    /// 递归重写类型名称，类型不受同名局部变量遮蔽；容器和可空后缀保留。
    fn rewrite_type(&self, ty: TypeName) -> TypeName {
        match ty {
            TypeName::Named(name) => TypeName::Named(self.rename_name(&name)),
            TypeName::Nullable(inner) => TypeName::Nullable(Box::new(self.rewrite_type(*inner))),
            TypeName::List(inner) => TypeName::List(Box::new(self.rewrite_type(*inner))),
            TypeName::Array(inner, n) => TypeName::Array(Box::new(self.rewrite_type(*inner)), n),
            TypeName::Map(k, v) => TypeName::Map(
                Box::new(self.rewrite_type(*k)),
                Box::new(self.rewrite_type(*v)),
            ),
            other => other,
        }
    }

    /// 重写函数参数或结构体字段中的类型，保持字段名及源码位置。
    fn rewrite_parameters(&self, fields: Vec<Parameter>) -> Vec<Parameter> {
        fields
            .into_iter()
            .map(|p| Parameter {
                type_name: self.rewrite_type(p.type_name),
                ..p
            })
            .collect()
    }

    /// 判断当前标识符是否应当映射到内部名，而不是保持原始文本。
    fn rewrite_identifier(&self, name: String) -> String {
        if self.is_local_name(&name) {
            name
        } else {
            self.rename_name(&name)
        }
    }

    /// 返回当前名称在模块内部对应的稳定重写结果，不存在时保留原名。
    fn rename_name(&self, name: &str) -> String {
        self.bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// 判断给定名称是否已在当前局部作用域中声明，从而不应被重写。
    fn is_local_name(&self, name: &str) -> bool {
        self.local_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    /// 返回当前可写局部作用域，用于登记新声明的局部变量名。
    fn current_scope_mut(&mut self) -> &mut HashSet<String> {
        self.local_scopes.last_mut().expect("局部作用域栈不能为空")
    }
}

fn collect_top_level_declarations(
    program: &Program,
    module_path: &Path,
    is_entry: bool,
) -> BTreeMap<String, String> {
    program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::StructDecl { name, .. }
            | Stmt::FunctionDecl { name, .. }
            | Stmt::VarDecl { name, .. } => Some((
                name.clone(),
                if is_entry && name == "main" {
                    "main".to_string()
                } else {
                    format!("__ms_{}_{}", sanitize_module_name(module_path), name)
                },
            )),
            Stmt::ExternFunctionDecl { name, .. } => Some((name.clone(), name.clone())),
            _ => None,
        })
        .collect()
}

/// 将选中的公开符号加入绑定表并返回本条语句提供的绑定，供重导出复用。
/// 同一可见名只允许重复指向同一符号；缺失成员、异源冲突或同条语句内重复的限定名会报错。
fn merge_imports(
    kind: &ImportKind,
    exports: &BTreeMap<String, String>,
    imports: &mut BTreeMap<String, String>,
    path: &ImportPath,
) -> Result<BTreeMap<String, String>, CompilerError> {
    let mut inserted = BTreeMap::new();
    let mut bind = |name: String, internal: &String| {
        if (path.alias.is_some() && inserted.contains_key(&name))
            || imports.get(&name).is_some_and(|previous| previous != internal)
        {
            return Err(CompilerError::build_message(
                format!("导入的成员名 `{name}` 重复"),
                path.span,
            ));
        }
        imports.insert(name.clone(), internal.clone());
        inserted.insert(name, internal.clone());
        Ok(())
    };
    match kind {
        ImportKind::All => {
            for (name, internal) in exports {
                bind(name.clone(), internal)?;
            }
        }
        ImportKind::Named(items) => {
            for ImportItem { name, alias, .. } in items {
                let internal = exports.get(name).ok_or_else(|| {
                    CompilerError::build_message(
                        format!("导入的符号 `{name}` 不存在或未公开"),
                        path.span,
                    )
                })?;
                let member = alias.as_ref().unwrap_or(name);
                let visible_name = match &path.alias {
                    Some(namespace) => format!("{namespace}.{member}"),
                    None => member.clone(),
                };
                bind(visible_name, internal)?;
            }
        }
    }
    Ok(inserted)
}

fn resolve_module_file(
    module_root: &Path,
    segments: &[String],
    span: Span,
) -> Result<PathBuf, CompilerError> {
    let mut path = module_root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    let direct_file = path.with_extension("ms");
    if direct_file.is_file() {
        return canonicalize_path(&direct_file);
    }
    let index_file = path.join("index.ms");
    if index_file.is_file() {
        return canonicalize_path(&index_file);
    }
    Err(CompilerError::build_message(
        format!("找不到模块文件：{}", path.display()),
        span,
    ))
}

fn append_module_source(output: &mut String, module_path: &Path, source: &SourceFile) -> usize {
    let header = format!("// module: {}\n", module_path.display());
    output.push_str(&header);
    let offset = output.len();
    output.push_str(source.text());
    if !source.text().ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    offset
}

fn read_module_source(path: &Path) -> Result<SourceFile, CompilerError> {
    let text = fs::read_to_string(path).map_err(|error| {
        CompilerError::build_message(
            format!("读取模块失败：{}: {error}", path.display()),
            Span::new(0, 0),
        )
    })?;
    Ok(SourceFile::new(path.display().to_string(), text))
}

/// 从入口所在目录向上寻找最近的项目设置文件。
fn discover_project_root(entry_path: &Path) -> Option<PathBuf> {
    let mut current = entry_path.parent();
    while let Some(path) = current {
        if path.join("settings.toml").is_file() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

/// 为入口或缓存模块定位所属包，找不到设置文件时返回构建诊断。
fn discover_package_root(
    module_path: &Path,
    project_root: &Path,
) -> Result<PathBuf, CompilerError> {
    let mut current = module_path.parent();
    while let Some(path) = current {
        if path.join("settings.toml").is_file()
            && (path == project_root || path.join("src").is_dir())
        {
            return Ok(path.to_path_buf());
        }
        if path == project_root {
            break;
        }
        current = path.parent();
    }
    Err(CompilerError::build_message(
        format!("无法定位模块所属包根目录：{}", module_path.display()),
        Span::new(0, 0),
    ))
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, CompilerError> {
    fs::canonicalize(path).map_err(|error| {
        CompilerError::build_message(
            format!("解析路径失败：{}: {error}", path.display()),
            Span::new(0, 0),
        )
    })
}

fn canonicalize_existing_path(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    fs::canonicalize(path).ok()
}

fn parse_single_source(
    source: &SourceFile,
    settings: &CompilerSettings,
) -> Result<ModuleLoadResult, CompilerError> {
    let tokens = Lexer::new(source, settings).tokenize()?;
    let program = Parser::new(tokens, settings).parse_program()?;
    Ok(ModuleLoadResult {
        origins: Vec::new(),
        source: source.clone(),
        program,
        package_roots: Vec::new(),
    })
}

fn sanitize_module_name(module_path: &Path) -> String {
    module_path
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// 平移模块语句和字段的源码位置，使诊断仍指向原始文件。
fn shift_stmt(statement: Stmt, offset: usize) -> Stmt {
    match statement {
        Stmt::StructDecl {
            is_public,
            name,
            fields,
            span,
        } => Stmt::StructDecl {
            is_public,
            name,
            fields: fields
                .into_iter()
                .map(|f| Parameter {
                    span: shift_span(f.span, offset),
                    ..f
                })
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::FunctionDecl {
            is_public,
            name,
            parameters,
            return_type,
            body,
            span,
        } => Stmt::FunctionDecl {
            is_public,
            name,
            parameters: parameters
                .into_iter()
                .map(|mut item| {
                    item.span = shift_span(item.span, offset);
                    item
                })
                .collect(),
            return_type,
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ExternFunctionDecl {
            is_public,
            name,
            parameters,
            return_type,
            span,
        } => Stmt::ExternFunctionDecl {
            is_public,
            name,
            parameters: parameters
                .into_iter()
                .map(|mut item| {
                    item.span = shift_span(item.span, offset);
                    item
                })
                .collect(),
            return_type,
            span: shift_span(span, offset),
        },
        Stmt::VarDecl {
            is_public,
            binding_kind,
            name,
            annotation,
            initializer,
            span,
        } => Stmt::VarDecl {
            is_public,
            binding_kind,
            name,
            annotation,
            initializer: initializer.map(|expr| shift_expr(expr, offset)),
            span: shift_span(span, offset),
        },
        Stmt::Assign {
            target,
            op,
            value,
            span,
        } => Stmt::Assign {
            target: shift_target(target, offset),
            op,
            value: shift_expr(value, offset),
            span: shift_span(span, offset),
        },
        Stmt::If {
            branches,
            else_branch,
            span,
        } => Stmt::If {
            branches: branches
                .into_iter()
                .map(|branch| ConditionalBranch {
                    condition: shift_expr(branch.condition, offset),
                    body: branch
                        .body
                        .into_iter()
                        .map(|item| shift_stmt(item, offset))
                        .collect(),
                    span: shift_span(branch.span, offset),
                })
                .collect(),
            else_branch: else_branch.map(|body| {
                body.into_iter()
                    .map(|item| shift_stmt(item, offset))
                    .collect()
            }),
            span: shift_span(span, offset),
        },
        Stmt::While {
            condition,
            body,
            span,
        } => Stmt::While {
            condition: shift_expr(condition, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ForRange {
            name,
            start,
            end,
            body,
            span,
        } => Stmt::ForRange {
            name,
            start: shift_expr(start, offset),
            end: shift_expr(end, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ForEach {
            key_name,
            value_name,
            iterable,
            body,
            span,
        } => Stmt::ForEach {
            key_name,
            value_name,
            iterable: shift_expr(iterable, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.map(|expr| shift_expr(expr, offset)),
            span: shift_span(span, offset),
        },
        Stmt::ExprStmt { expr, span } => Stmt::ExprStmt {
            expr: shift_expr(expr, offset),
            span: shift_span(span, offset),
        },
        other => other,
    }
}

/// 平移赋值目标及嵌套表达式的位置，不改变字段名称。
fn shift_target(target: AssignTarget, offset: usize) -> AssignTarget {
    match target {
        AssignTarget::Field { target, name, span } => AssignTarget::Field {
            target: Box::new(shift_expr(*target, offset)),
            name,
            span: shift_span(span, offset),
        },
        AssignTarget::Identifier { name, span } => AssignTarget::Identifier {
            name,
            span: shift_span(span, offset),
        },
        AssignTarget::Index {
            target,
            index,
            span,
        } => AssignTarget::Index {
            target: Box::new(shift_expr(*target, offset)),
            index: Box::new(shift_expr(*index, offset)),
            span: shift_span(span, offset),
        },
    }
}

/// 平移表达式位置，包括结构体成员访问，供合并模块后的诊断使用。
fn shift_expr(expr: Expr, offset: usize) -> Expr {
    match expr {
        Expr::FieldAccess { target, name, span } => Expr::FieldAccess {
            target: Box::new(shift_expr(*target, offset)),
            name,
            span: shift_span(span, offset),
        },
        Expr::Literal { value, span } => Expr::Literal {
            value,
            span: shift_span(span, offset),
        },
        Expr::Identifier { name, span } => Expr::Identifier {
            name,
            span: shift_span(span, offset),
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => Expr::Binary {
            left: Box::new(shift_expr(*left, offset)),
            op,
            right: Box::new(shift_expr(*right, offset)),
            span: shift_span(span, offset),
        },
        Expr::Unary { op, operand, span } => Expr::Unary {
            op,
            operand: Box::new(shift_expr(*operand, offset)),
            span: shift_span(span, offset),
        },
        Expr::Call {
            callee,
            arguments,
            span,
        } => Expr::Call {
            callee: Box::new(shift_expr(*callee, offset)),
            arguments: arguments
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::ListLiteral { elements, span } => Expr::ListLiteral {
            elements: elements
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::MapLiteral { entries, span } => Expr::MapLiteral {
            entries: entries
                .into_iter()
                .map(|entry| MapEntry {
                    key: shift_expr(entry.key, offset),
                    value: shift_expr(entry.value, offset),
                    span: shift_span(entry.span, offset),
                })
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::ArrayInitializer {
            element_type,
            size,
            elements,
            span,
        } => Expr::ArrayInitializer {
            element_type,
            size,
            elements: elements
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::Index {
            target,
            index,
            span,
        } => Expr::Index {
            target: Box::new(shift_expr(*target, offset)),
            index: Box::new(shift_expr(*index, offset)),
            span: shift_span(span, offset),
        },
        Expr::Slice {
            target,
            start,
            end,
            span,
        } => Expr::Slice {
            target: Box::new(shift_expr(*target, offset)),
            start: start.map(|expr| Box::new(shift_expr(*expr, offset))),
            end: end.map(|expr| Box::new(shift_expr(*expr, offset))),
            span: shift_span(span, offset),
        },
        Expr::MethodCall {
            target,
            method,
            arguments,
            span,
        } => Expr::MethodCall {
            target: Box::new(shift_expr(*target, offset)),
            method,
            arguments: arguments
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::PropertyAccess {
            target,
            property,
            span,
        } => Expr::PropertyAccess {
            target: Box::new(shift_expr(*target, offset)),
            property,
            span: shift_span(span, offset),
        },
        Expr::Grouped { expression, span } => Expr::Grouped {
            expression: Box::new(shift_expr(*expression, offset)),
            span: shift_span(span, offset),
        },
    }
}

fn shift_span(span: Span, offset: usize) -> Span {
    Span::new(span.start + offset, span.end + offset)
}
