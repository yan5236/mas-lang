const fs = require('fs');
const crypto = require('crypto');
const path = require('path');

const rootDirectory = path.resolve(__dirname, '..');
const defaultSource = path.resolve(rootDirectory, '..', '..', 'target', 'debug', 'mas-lang-lsp.exe');
const sourcePath = path.resolve(process.env.MAS_LANG_LSP_EXE || defaultSource);
const targetDirectory = path.join(rootDirectory, 'server');
const targetPath = path.join(targetDirectory, 'mas-lang-lsp.exe');

/**
 * 复制本地构建好的 mas-lang LSP server 到 VS Code 扩展的内置 server 目录。
 */
function copyServer() {
	if (!fs.existsSync(sourcePath)) {
		throw new Error(`找不到 mas-lang LSP server 构建产物：${sourcePath}`);
	}

	fs.mkdirSync(targetDirectory, { recursive: true });

	if (fs.existsSync(targetPath) && fileHash(sourcePath) === fileHash(targetPath)) {
		console.log(`mas-lang LSP server 已是最新：${targetPath}`);
		return;
	}

	fs.copyFileSync(sourcePath, targetPath);
	console.log(`已复制 mas-lang LSP server：${sourcePath} -> ${targetPath}`);
}

/**
 * 计算文件哈希，用于避免重复覆盖正在被扩展宿主占用的相同 server 文件。
 */
function fileHash(filePath) {
	return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

copyServer();
