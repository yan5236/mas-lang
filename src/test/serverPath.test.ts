import * as assert from 'assert';
import * as fs from 'fs/promises';
import * as os from 'os';
import * as path from 'path';

import { ServerOptions } from 'vscode-languageclient/node';

import { createBundledServerOptions, getBundledServerPath } from '../serverPath';

type RunDebugServerOptions = {
	run: { command: string };
	debug: { command: string };
};

suite('Mascript LSP Server Path Test Suite', () => {
	/**
	 * 创建一个临时扩展目录，用于模拟 VS Code 安装后的插件根目录。
	 */
	async function createExtensionDirectory(): Promise<string> {
		return fs.mkdtemp(path.join(os.tmpdir(), 'mas-lang-vscode-extension-'));
	}

	test('should resolve bundled server path inside extension server directory', async () => {
		const extensionDirectory = await createExtensionDirectory();
		const serverPath = getBundledServerPath(extensionDirectory);

		assert.strictEqual(serverPath, path.join(extensionDirectory, 'server', 'mas-lang-lsp.exe'));
	});

	test('should create stdio executable server options for bundled server', async () => {
		const extensionDirectory = await createExtensionDirectory();
		const serverPath = getBundledServerPath(extensionDirectory);
		const serverOptions = createBundledServerOptions(serverPath);

		assert.ok(hasRunDebugOptions(serverOptions));
		assert.deepStrictEqual(serverOptions.run, { command: serverPath });
		assert.deepStrictEqual(serverOptions.debug, { command: serverPath });
	});
});

/**
 * 收窄 LanguageClient 的 ServerOptions 联合类型，便于测试 run/debug 可执行文件配置。
 */
function hasRunDebugOptions(serverOptions: ServerOptions): serverOptions is RunDebugServerOptions {
	return 'run' in serverOptions && 'debug' in serverOptions;
}
