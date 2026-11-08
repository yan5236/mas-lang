import * as path from 'path';

import { ServerOptions } from 'vscode-languageclient/node';

const SERVER_DIRECTORY = 'server';
const SERVER_EXECUTABLE = 'mas-lang-lsp.exe';

/**
 * 根据扩展安装根目录解析内置 mas-lang LSP server 的绝对路径。
 */
export function getBundledServerPath(extensionPath: string): string {
	return path.join(extensionPath, SERVER_DIRECTORY, SERVER_EXECUTABLE);
}

/**
 * 为 VS Code LanguageClient 创建使用标准输入输出的本地可执行文件启动配置。
 */
export function createBundledServerOptions(serverPath: string): ServerOptions {
	return {
		run: { command: serverPath },
		debug: { command: serverPath },
	};
}
