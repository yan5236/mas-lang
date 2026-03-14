import * as assert from 'assert';
import * as fs from 'fs/promises';
import * as os from 'os';
import * as path from 'path';

import * as vscode from 'vscode';

suite('Mascript Extension Test Suite', () => {
	/**
	 * 为每个测试用例创建临时 `.ms` 文件并返回对应文档对象。
	 */
	async function openMascriptDocument(content: string): Promise<vscode.TextDocument> {
		const tempDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'mascript-lang-'));
		const filePath = path.join(tempDirectory, 'sample.ms');
		await fs.writeFile(filePath, content, 'utf8');
		const document = await vscode.workspace.openTextDocument(vscode.Uri.file(filePath));
		await vscode.window.showTextDocument(document);
		return document;
	}

	/**
	 * 调用 VS Code 的补全命令，并把结果统一转成字符串标签列表。
	 */
	async function getCompletionLabels(
		document: vscode.TextDocument,
		position: vscode.Position,
	): Promise<string[]> {
		const completionList = await vscode.commands.executeCommand<vscode.CompletionList>(
			'vscode.executeCompletionItemProvider',
			document.uri,
			position,
		);

		return (completionList?.items ?? []).map((item) =>
			typeof item.label === 'string' ? item.label : item.label.label,
		);
	}

	test('should detect mascript language for ms file', async () => {
		const document = await openMascriptDocument('func main() :\n    print("hello")\n');
		assert.strictEqual(document.languageId, 'mascript');
	});

	test('should provide type completions in annotation context', async () => {
		const document = await openMascriptDocument('var value :\n');
		const labels = await getCompletionLabels(document, new vscode.Position(0, 'var value :'.length));

		assert.ok(labels.includes('int'));
		assert.ok(labels.includes('float'));
		assert.ok(labels.includes('bool'));
		assert.ok(labels.includes('string'));
		assert.ok(!labels.includes('print'));
	});

	test('should provide statement completions at line start', async () => {
		const document = await openMascriptDocument('\n');
		const labels = await getCompletionLabels(document, new vscode.Position(0, 0));

		assert.ok(labels.includes('func'));
		assert.ok(labels.includes('if'));
		assert.ok(labels.includes('while'));
		assert.ok(labels.includes('for'));
		assert.ok(labels.includes('var'));
		assert.ok(labels.includes('const'));
	});

	test('should provide expression completions inside assignment value', async () => {
		const document = await openMascriptDocument('func main() :\n    var result = \n');
		const labels = await getCompletionLabels(
			document,
			new vscode.Position(1, '    var result = '.length),
		);

		assert.ok(labels.includes('print'));
		assert.ok(labels.includes('true'));
		assert.ok(labels.includes('false'));
		assert.ok(labels.includes('not'));
	});
});
