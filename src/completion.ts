import * as vscode from 'vscode';

type CompletionContextKind = 'type' | 'statement' | 'expression';

const TYPE_NAMES = ['int', 'float', 'bool', 'string'] as const;
const KEYWORDS = [
	'var',
	'const',
	'func',
	'if',
	'elif',
	'else',
	'while',
	'for',
	'in',
	'break',
	'continue',
	'return',
	'and',
	'or',
	'not',
] as const;
const BOOLEAN_LITERALS = ['true', 'false'] as const;

/**
 * 创建 Mascript 语言的补全提供器，并把补全规则封装为独立模块。
 */
export function createMascriptCompletionProvider(): vscode.CompletionItemProvider {
	return {
		provideCompletionItems(document, position) {
			return buildCompletionItems(document, position);
		},
	};
}

/**
 * 根据光标上下文生成当前可用的补全项列表。
 */
function buildCompletionItems(
	document: vscode.TextDocument,
	position: vscode.Position,
): vscode.CompletionItem[] {
	const context = detectCompletionContext(document, position);
	const range = resolveCompletionRange(document, position);

	if (context === 'type') {
		return TYPE_NAMES.map((typeName) => createTypeItem(typeName, range));
	}

	if (context === 'statement') {
		return [
			...KEYWORDS.map((keyword) => createKeywordItem(keyword, range)),
			createSnippetItem('func', '函数骨架', 'func ${1:name}(${2}) :\n    ${0}', range),
			createSnippetItem('if', 'if 代码块', 'if ${1:condition} :\n    ${0}', range),
			createSnippetItem('elif', 'elif 代码块', 'elif ${1:condition} :\n    ${0}', range),
			createSnippetItem('else', 'else 代码块', 'else :\n    ${0}', range),
			createSnippetItem('while', 'while 循环', 'while ${1:condition} :\n    ${0}', range),
			createSnippetItem('for', 'for 范围循环', 'for ${1:item} in ${2:0}..${3:10} :\n    ${0}', range),
			createSnippetItem('main', 'main 函数', 'func main() :\n    ${0}', range),
		];
	}

	return [
		createBuiltinFunctionItem('print', '内建输出函数', 'print(${1:value})', range),
		...BOOLEAN_LITERALS.map((literal) => createLiteralItem(literal, range)),
		createKeywordItem('not', range),
		createKeywordItem('and', range),
		createKeywordItem('or', range),
		createKeywordItem('return', range),
		createKeywordItem('break', range),
		createKeywordItem('continue', range),
	];
}

/**
 * 判断当前光标所处位置应该走类型补全、语句补全还是表达式补全。
 */
function detectCompletionContext(
	document: vscode.TextDocument,
	position: vscode.Position,
): CompletionContextKind {
	const linePrefix = document.lineAt(position.line).text.slice(0, position.character);

	if (isTypeAnnotationContext(linePrefix)) {
		return 'type';
	}

	if (isStatementContext(linePrefix)) {
		return 'statement';
	}

	return 'expression';
}

/**
 * 判断当前行前缀是否处于类型标注或返回类型位置。
 */
function isTypeAnnotationContext(linePrefix: string): boolean {
	return (
		/(?:->)\s*[A-Za-z_]*$/.test(linePrefix) ||
		/\b(?:var|const)\s+[A-Za-z_][A-Za-z0-9_]*\s*:\s*[A-Za-z_]*$/.test(linePrefix) ||
		/\bfunc\s+[A-Za-z_][A-Za-z0-9_]*\s*\([^)]*\b[A-Za-z_][A-Za-z0-9_]*\s*:\s*[A-Za-z_]*$/.test(
			linePrefix,
		)
	);
}

/**
 * 判断当前行前缀是否仍处于一条新语句的开头位置。
 */
function isStatementContext(linePrefix: string): boolean {
	return /^\s*[A-Za-z_]*$/.test(linePrefix);
}

/**
 * 解析当前单词范围，保证补全时会替换掉用户已经输入的前缀。
 */
function resolveCompletionRange(
	document: vscode.TextDocument,
	position: vscode.Position,
): vscode.Range | undefined {
	return document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
}

/**
 * 构造关键字类补全项，统一设置基础元数据。
 */
function createKeywordItem(
	keyword: string,
	range: vscode.Range | undefined,
): vscode.CompletionItem {
	const item = new vscode.CompletionItem(keyword, vscode.CompletionItemKind.Keyword);
	item.detail = 'Mascript 关键字';
	item.range = range;
	return item;
}

/**
 * 构造内建类型补全项，便于在类型标注上下文中使用。
 */
function createTypeItem(
	typeName: string,
	range: vscode.Range | undefined,
): vscode.CompletionItem {
	const item = new vscode.CompletionItem(typeName, vscode.CompletionItemKind.TypeParameter);
	item.detail = 'Mascript 内建类型';
	item.range = range;
	return item;
}

/**
 * 构造布尔字面量补全项，用于表达式位置快速插入。
 */
function createLiteralItem(
	literal: string,
	range: vscode.Range | undefined,
): vscode.CompletionItem {
	const item = new vscode.CompletionItem(literal, vscode.CompletionItemKind.Value);
	item.detail = 'Mascript 布尔字面量';
	item.range = range;
	return item;
}

/**
 * 构造内建函数补全项，并以 snippet 形式补齐括号。
 */
function createBuiltinFunctionItem(
	label: string,
	detail: string,
	snippet: string,
	range: vscode.Range | undefined,
): vscode.CompletionItem {
	const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Function);
	item.detail = detail;
	item.insertText = new vscode.SnippetString(snippet);
	item.range = range;
	return item;
}

/**
 * 构造代码骨架类补全项，统一使用 snippet 插入多行模板。
 */
function createSnippetItem(
	label: string,
	detail: string,
	snippet: string,
	range: vscode.Range | undefined,
): vscode.CompletionItem {
	const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Snippet);
	item.detail = detail;
	item.insertText = new vscode.SnippetString(snippet);
	item.range = range;
	return item;
}
