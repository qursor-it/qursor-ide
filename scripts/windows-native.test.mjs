import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { windowsConfigProblems } from './windows-native.mjs';

const windowsConfig = JSON.parse(
	await readFile(
		new URL('../apps/desktop/src-tauri/tauri.windows.conf.json', import.meta.url),
		'utf8',
	),
);
// The real key is pasted by the owner; fixtures use a synthetic one.
windowsConfig.plugins.updater.pubkey = 'synthetic-pubkey';
const appConfig = JSON.parse(
	await readFile(new URL('../apps/desktop/src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
);

test('Windows native configuration creates only an NSIS bundle without updater artifacts', () => {
	assert.deepEqual(windowsConfigProblems(windowsConfig, appConfig), []);
});

test('native product metadata is branded while the app data identity stays compatible', () => {
	assert.equal(appConfig.productName, 'qursor-ide');
	assert.equal(appConfig.identifier, 'dev.factorai');
	assert.ok(appConfig.app.windows.some((window) => window.title === 'Qursor IDE'));

	const renamed = structuredClone(appConfig);
	renamed.productName = 'factorai';
	renamed.identifier = 'dev.qursor-ide';
	renamed.app.windows[0].title = 'factorai';
	assert.deepEqual(windowsConfigProblems(windowsConfig, renamed), [
		'Tauri productName must be "qursor-ide"',
		'Tauri identifier must remain "dev.factorai"',
		'Tauri window title must be "Qursor IDE"',
	]);
});

test('Windows overlay cannot replace the product name or stable release identity', () => {
	const config = structuredClone(windowsConfig);
	config.productName = 'factorai';
	config.identifier = 'dev.qursor-ide';
	assert.deepEqual(windowsConfigProblems(config, appConfig), [
		'Windows productName override must be "qursor-ide"',
		'Windows identifier override must remain "dev.factorai"',
	]);
});

test('Windows native configuration fails closed if the updater feed or key is wrong', () => {
	const config = structuredClone(windowsConfig);
	config.plugins.updater.endpoints = ['https://example.invalid/latest.json'];
	config.plugins.updater.pubkey = 'REPLACE_WITH_QURSOR_UPDATER_PUBKEY';

	assert.deepEqual(windowsConfigProblems(config, appConfig), [
		'plugins.updater.endpoints must be exactly [https://github.com/qursor-it/qursor-ide/releases/latest/download/latest.json]',
		'plugins.updater.pubkey must be the Qursor updater public key',
	]);
	config.plugins.updater.pubkey = '';
	assert.equal(windowsConfigProblems(config, appConfig).length, 2);
});

test('source builds keep an empty updater feed in the base configuration', () => {
	const app = structuredClone(appConfig);
	app.plugins.updater.endpoints = ['https://example.invalid/latest.json'];
	assert.deepEqual(windowsConfigProblems(windowsConfig, app), [
		'tauri.conf.json plugins.updater.endpoints must be an empty array',
	]);
});

test('Windows native configuration fails closed if a non-NSIS target or updater artifacts are enabled', () => {
	const config = structuredClone(windowsConfig);
	config.bundle.targets = ['nsis', 'msi'];
	config.bundle.createUpdaterArtifacts = true;

	assert.deepEqual(windowsConfigProblems(config, appConfig), [
		'bundle.targets must contain only "nsis"',
		'bundle.createUpdaterArtifacts must be false',
	]);
});

test('Windows installer keeps the hook that removes the pre-rename factorai install', () => {
	const config = structuredClone(windowsConfig);
	delete config.bundle.windows;
	assert.deepEqual(windowsConfigProblems(config, appConfig), [
		'bundle.windows.nsis.installerHooks must be "./windows/hooks.nsh"',
	]);
});
