import * as vscode from 'vscode';
import { LanguageClient, LanguageClientOptions, RevealOutputChannelOn } from 'vscode-languageclient/node';

import { createBundledServerOptions, getBundledServerPath } from './serverPath';

const MASCRIPT_DOCUMENT_FILTER = { language: 'mascript', scheme: 'file', pattern: '**/*.ms' };
const SERVER_MISSING_MESSAGE = '当前插件缺少 mas-lang LSP server，请重新构建或安装带 server 的版本。';

let client: LanguageClient | undefined;

/**
 * 激活扩展时启动内置 mas-lang LSP server，让诊断、补全、悬停和文档符号统一由语言服务器提供。
 */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
	const serverPath = getBundledServerPath(context.extensionPath);

	if (!await fileExists(serverPath)) {
		await vscode.window.showWarningMessage(SERVER_MISSING_MESSAGE);
		return;
	}

	client = new LanguageClient(
		'mas-lang-lsp',
		'mas-lang Language Server',
		createBundledServerOptions(serverPath),
		createClientOptions(),
	);

	context.subscriptions.push(client);
	await client.start();
}

/**
 * 扩展停用时停止 LSP 客户端，确保 VS Code 关闭扩展宿主时同步回收 server 进程。
 */
export async function deactivate(): Promise<void> {
	await client?.stop();
	client = undefined;
}

/**
 * 创建 LanguageClient 配置，限定只处理本地 `.ms` 文件对应的 mascript 文档。
 */
function createClientOptions(): LanguageClientOptions {
	return {
		documentSelector: [MASCRIPT_DOCUMENT_FILTER],
		revealOutputChannelOn: RevealOutputChannelOn.Error,
	};
}

/**
 * 以 VS Code 文件系统 API 检查内置 server 是否存在，兼容扩展安装目录中的普通文件访问。
 */
async function fileExists(filePath: string): Promise<boolean> {
	try {
		await vscode.workspace.fs.stat(vscode.Uri.file(filePath));
		return true;
	} catch {
		return false;
	}
}
