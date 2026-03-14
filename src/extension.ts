import * as vscode from 'vscode';

import { createMascriptCompletionProvider } from './completion';

const MASCRIPT_SELECTOR: vscode.DocumentSelector = { language: 'mascript', scheme: 'file' };

/**
 * 激活扩展时注册 Mascript 的补全提供器。
 */
export function activate(context: vscode.ExtensionContext): void {
	const completionProvider = vscode.languages.registerCompletionItemProvider(
		MASCRIPT_SELECTOR,
		createMascriptCompletionProvider(),
		':',
		'(',
	);

	context.subscriptions.push(completionProvider);
}

/**
 * 扩展停用时无需额外清理，保留空实现以符合 VS Code 扩展入口约定。
 */
export function deactivate(): void {}
