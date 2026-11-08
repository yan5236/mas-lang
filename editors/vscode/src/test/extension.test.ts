import * as assert from 'assert';
import * as fs from 'fs/promises';
import * as os from 'os';
import * as path from 'path';

import * as vscode from 'vscode';

const EXTENSION_ROOT = path.resolve(__dirname, '..', '..');
const LANGUAGE_CONFIGURATION_PATH = path.join(EXTENSION_ROOT, 'language-configuration.json');

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

	test('should detect mascript language for ms file', async () => {
		const document = await openMascriptDocument('func main() :\n    print("hello")\n');
		assert.strictEqual(document.languageId, 'mascript');
	});

	test('should keep mascript file content available for language server', async () => {
		const source = 'func main() :\n    print("hello")\n';
		const document = await openMascriptDocument(source);

		assert.strictEqual(document.getText(), source);
	});

	test('should declare colon block on-enter indentation rules', async () => {
		const rawConfiguration = await fs.readFile(LANGUAGE_CONFIGURATION_PATH, 'utf8');
		const configuration = JSON.parse(rawConfiguration) as {
			indentationRules?: {
				decreaseIndentPattern?: string;
			};
			onEnterRules?: Array<{
				beforeText?: string;
				action?: { indent?: string };
			}>;
		};

		assert.ok(configuration.onEnterRules?.some((rule) => {
			return rule.action?.indent === 'indent'
				&& typeof rule.beforeText === 'string'
				&& rule.beforeText.includes('func')
				&& rule.beforeText.includes('if')
				&& rule.beforeText.includes(':');
		}));
		assert.notStrictEqual(configuration.indentationRules?.decreaseIndentPattern, '^\\s*$');
	});
});
