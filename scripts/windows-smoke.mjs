// Smoke the real native Windows WebView2 bridge and PowerShell ConPTY path.
// See ADR-0074 and docs/windows-native-validation.md for the isolated launch.
// The FactorAI-namespaced smoke identifier is an internal safety boundary; ADR-0075 keeps it stable.
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

const port = 9230;
const expectedIdentifier = 'it.qursor.ide-windows-smoke';
const powershellMarkerPattern = /FACTORAI_WINDOWS_SMOKE:(Desktop|Core):[A-Z]:\\/i;
const pending = new Map();
const cleanupWarnings = [];
const report = {
	identifier: null,
	fileRoundTrip: false,
	powershellPty: false,
	terminalCleanup: null,
	remainingTerminals: null,
};
let socket;
let sequence = 0;
let identityVerified = false;
let fixturePath;
let projectId;
let terminalId;
let core;
let failure;

async function connect() {
	const response = await fetch(`http://127.0.0.1:${port}/json/list`);
	if (!response.ok) throw new Error(`WebView2 CDP returned HTTP ${response.status}`);
	const targets = await response.json();
	const page = targets.find((target) => target.type === 'page' && target.webSocketDebuggerUrl);
	if (!page) throw new Error(`No WebView2 page is listening on CDP port ${port}`);

	socket = new WebSocket(page.webSocketDebuggerUrl);
	await new Promise((resolveOpen, rejectOpen) => {
		const timeout = setTimeout(
			() => rejectOpen(new Error('Timed out opening WebView2 CDP')),
			15000,
		);
		socket.addEventListener(
			'open',
			() => {
				clearTimeout(timeout);
				resolveOpen();
			},
			{ once: true },
		);
		socket.addEventListener(
			'error',
			() => {
				clearTimeout(timeout);
				rejectOpen(new Error('Could not open WebView2 CDP'));
			},
			{ once: true },
		);
	});
	socket.addEventListener('message', (event) => {
		const message = JSON.parse(String(event.data));
		if (!message.id) return;
		const request = pending.get(message.id);
		if (!request) return;
		pending.delete(message.id);
		if (message.error) request.reject(new Error(JSON.stringify(message.error)));
		else request.resolve(message.result);
	});
	socket.addEventListener('close', () => {
		for (const request of pending.values()) request.reject(new Error('WebView2 CDP closed'));
		pending.clear();
	});
}

function send(method, params = {}, timeoutMs = 20000) {
	if (socket?.readyState !== WebSocket.OPEN)
		return Promise.reject(new Error('WebView2 CDP is not connected'));
	const id = ++sequence;
	return new Promise((resolveRequest, rejectRequest) => {
		const timeout = setTimeout(() => {
			pending.delete(id);
			rejectRequest(new Error(`Timed out waiting for ${method}`));
		}, timeoutMs);
		pending.set(id, {
			resolve(value) {
				clearTimeout(timeout);
				resolveRequest(value);
			},
			reject(error) {
				clearTimeout(timeout);
				rejectRequest(error);
			},
		});
		socket.send(JSON.stringify({ id, method, params }));
	});
}

async function evaluate(expression, timeoutMs = 20000) {
	const result = await send(
		'Runtime.evaluate',
		{
			expression,
			awaitPromise: true,
			returnByValue: true,
		},
		timeoutMs,
	);
	if (result.exceptionDetails) {
		throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
	}
	return result.result?.value;
}

async function invoke(command, args = {}, timeoutMs = 30000) {
	return evaluate(
		`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
		timeoutMs,
	);
}

const pause = (ms) => new Promise((resolvePause) => setTimeout(resolvePause, ms));

/** A client of qursor-core, which owns every terminal: the app hands out its
 *  endpoint, and the wire is JSON for control and binary frames
 *  `opcode | slot (u16 BE) | bytes` for terminal bytes
 *  (docs/superpowers/specs/2026-11-26-local-core-design.md §3). */
async function openCore({ url, secret }) {
	const ws = new WebSocket(url);
	ws.binaryType = 'arraybuffer';
	const calls = new Map();
	const outputs = new Map();
	let next = 1;
	let ready;
	const handshake = new Promise((resolveReady, rejectReady) => {
		ready = { resolve: resolveReady, reject: rejectReady };
	});
	ws.addEventListener('open', () =>
		ws.send(
			JSON.stringify({
				type: 'hello',
				secret,
				clientId: `windows-smoke-${process.pid}`,
				clientType: 'cli',
				protocolVersion: 1,
				appVersion: 'windows-smoke',
				capabilities: [],
			}),
		),
	);
	ws.addEventListener('message', (event) => {
		if (typeof event.data !== 'string') {
			const bytes = new Uint8Array(event.data);
			const slot = (bytes[1] << 8) | bytes[2];
			if (bytes[0] === 1) outputs.get(slot)?.(new TextDecoder().decode(bytes.subarray(3)));
			return;
		}
		const msg = JSON.parse(event.data);
		if (msg.type === 'server_info') {
			if (msg.protocolVersion === 1) ready.resolve();
			else ready.reject(new Error(`qursor-core speaks protocol ${msg.protocolVersion}`));
		} else if (msg.type === 'response' || msg.type === 'rpc_error') {
			const call = calls.get(msg.requestId);
			calls.delete(msg.requestId);
			if (msg.type === 'response') call?.resolve(msg.result);
			else call?.reject(new Error(JSON.stringify(msg.error)));
		}
	});
	ws.addEventListener('close', () => {
		ready.reject(new Error('qursor-core closed the connection'));
		for (const call of calls.values()) call.reject(new Error('qursor-core closed the connection'));
		calls.clear();
	});
	await handshake;
	return {
		request(method, params = null) {
			const requestId = next++;
			return new Promise((resolveCall, rejectCall) => {
				calls.set(requestId, { resolve: resolveCall, reject: rejectCall });
				ws.send(JSON.stringify({ type: 'request', requestId, method, params }));
			});
		},
		onOutput(slot, cb) {
			outputs.set(slot, cb);
		},
		write(slot, text) {
			const data = new TextEncoder().encode(text);
			const frame = new Uint8Array(3 + data.length);
			frame[0] = 2;
			frame[1] = slot >> 8;
			frame[2] = slot & 0xff;
			frame.set(data, 3);
			ws.send(frame);
		},
		close() {
			ws.close();
		},
	};
}

async function waitForTerminalCleanup(id, timeoutMs = 20000) {
	const deadline = Date.now() + timeoutMs;
	let terminals = [];
	while (Date.now() < deadline) {
		terminals = await core.request('terminal.list');
		if (!terminals.some((terminal) => terminal.id === id)) {
			report.remainingTerminals = terminals.length;
			if (terminals.length !== 0) {
				throw new Error(
					'Expected no remaining terminals; found ' +
						terminals.map((terminal) => terminal.id).join(', '),
				);
			}
			report.terminalCleanup = true;
			return;
		}
		await pause(200);
	}
	throw new Error('Terminal ' + id + ' remained in terminal.list after kill');
}

async function run() {
	await connect();
	const bridge = await evaluate(`({
    invoke: typeof window.__TAURI_INTERNALS__?.invoke,
    transformCallback: typeof window.__TAURI_INTERNALS__?.transformCallback,
    globalTauri: typeof window.__TAURI__,
  })`);
	if (
		bridge?.invoke !== 'function' ||
		bridge.transformCallback !== 'function' ||
		bridge.globalTauri !== 'undefined'
	) {
		throw new Error('Expected Tauri internals with withGlobalTauri=false');
	}

	// This read-only identity check is the gate for every filesystem and app mutation below.
	const identifier = await invoke('plugin:app|identifier');
	report.identifier = identifier;
	if (identifier !== expectedIdentifier) {
		throw new Error(
			`Refusing to mutate app ${JSON.stringify(identifier)}; expected ${expectedIdentifier}`,
		);
	}
	identityVerified = true;

	fixturePath = await mkdtemp(join(tmpdir(), 'factorai Windows \u00e8 smoke-'));
	const filePath = join(fixturePath, 'smoke \u00e8 file.txt');
	await writeFile(filePath, 'native Windows smoke, before\n', 'utf8');
	const project = await invoke('add_project', { path: fixturePath });
	projectId = project.id;
	if (!projectId) throw new Error('add_project returned no synthetic project id');

	const before = await invoke('read_file', { path: filePath });
	if (before.contents !== 'native Windows smoke, before\n')
		throw new Error('read_file returned unexpected fixture contents');
	const saved = await invoke('write_file', {
		path: filePath,
		contents: 'native Windows smoke, after\n',
	});
	const after = await invoke('read_file', { path: filePath });
	if (saved.contents !== after.contents || after.contents !== 'native Windows smoke, after\n') {
		throw new Error('write_file/read_file round-trip did not preserve the synthetic contents');
	}
	report.fileRoundTrip = true;

	core = await openCore(await invoke('core_endpoint'));
	const shell = await core.request('shell.spawn', {
		clientKey: `shell:windows-smoke-${process.pid}`,
		projectId,
		cwd: fixturePath,
		cols: 100,
		rows: 30,
	});
	terminalId = shell.id;
	let output = '';
	core.onOutput(shell.slot, (text) => {
		output += text;
	});
	core.write(
		shell.slot,
		"Write-Output ('FACTORAI_WINDOWS_SMOKE:' + $PSVersionTable.PSEdition + ':' + (Get-Location).Path)\r",
	);
	const deadline = Date.now() + 30000;
	while (!powershellMarkerPattern.test(output)) {
		if (Date.now() > deadline) throw new Error(`Timed out waiting for the marker in ${output}`);
		await pause(200);
	}
	const marker = output.split(/\r?\n/).find((line) => powershellMarkerPattern.test(line)) || '';
	if (
		!powershellMarkerPattern.test(marker) ||
		!marker.toLowerCase().includes(basename(fixturePath).toLowerCase())
	) {
		throw new Error(`PowerShell PTY returned no expected marker in ${fixturePath}: ${output}`);
	}
	report.powershellPty = true;
	report.powerShellMarker = marker;
}

async function cleanupInvoke(command, args) {
	try {
		await invoke(command, args);
	} catch (error) {
		cleanupWarnings.push(`${command}: ${String(error)}`);
	}
}

try {
	await run();
} catch (error) {
	failure = error;
} finally {
	if (identityVerified) {
		if (terminalId && projectId) {
			try {
				await core.request('terminal.kill', { id: terminalId });
			} catch (error) {
				try {
					await core.request('shell.killForProject', { projectId });
				} catch {
					cleanupWarnings.push(`terminal.kill: ${String(error)}`);
				}
			}
			try {
				await waitForTerminalCleanup(terminalId);
			} catch (error) {
				report.terminalCleanup = false;
				cleanupWarnings.push('terminal cleanup verification: ' + String(error));
			}
		}
		if (projectId) await cleanupInvoke('remove_project', { id: projectId });
	}
	if (fixturePath) {
		try {
			await rm(fixturePath, { recursive: true, force: true });
		} catch (error) {
			cleanupWarnings.push(`fixture cleanup: ${String(error)}`);
		}
	}
	core?.close();
	try {
		socket?.close();
	} catch (error) {
		cleanupWarnings.push(`CDP close: ${String(error)}`);
	}
}

report.cleanupWarnings = cleanupWarnings;
if (failure) report.failure = String(failure);
console.log(JSON.stringify(report, null, 2));
if (failure || cleanupWarnings.length) process.exitCode = 1;
