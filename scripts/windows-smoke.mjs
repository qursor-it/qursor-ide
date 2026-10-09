// Smoke the real native Windows WebView2 bridge and PowerShell ConPTY path.
// See ADR-0074 and docs/windows-native-validation.md for the isolated launch.
// The FactorAI-namespaced smoke identifier is an internal safety boundary; ADR-0075 keeps it stable.
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

const port = 9230;
const expectedIdentifier = 'dev.factorai-windows-smoke';
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
let eventId;
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

async function waitFor(expression, timeoutMs = 30000) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (await evaluate(expression)) return;
		await pause(200);
	}
	throw new Error(`Timed out waiting for ${expression}`);
}

async function waitForTerminalCleanup(id, timeoutMs = 20000) {
	const deadline = Date.now() + timeoutMs;
	let terminals = [];
	while (Date.now() < deadline) {
		terminals = await invoke('terminal_list');
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
	throw new Error('Terminal ' + id + ' remained in terminal_list after kill');
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

	eventId = await evaluate(`(() => {
    window.__windowsSmokeOutput = "";
    const handler = window.__TAURI_INTERNALS__.transformCallback((event) => {
      if (event.payload.id !== window.__windowsSmokeTerminalId) return;
      const binary = atob(event.payload.bytesB64);
      const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
      window.__windowsSmokeOutput += new TextDecoder().decode(bytes);
    });
    return window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
      event: "terminal:data", target: { kind: "Any" }, handler,
    });
  })()`);

	terminalId = await invoke('shell_spawn', {
		opts: {
			clientKey: `shell:windows-smoke-${process.pid}`,
			projectId,
			cwd: fixturePath,
			cols: 100,
			rows: 30,
		},
	});
	await evaluate(`window.__windowsSmokeTerminalId = ${JSON.stringify(terminalId)}; true`);
	await invoke('terminal_write', {
		id: terminalId,
		data: "Write-Output ('FACTORAI_WINDOWS_SMOKE:' + $PSVersionTable.PSEdition + ':' + (Get-Location).Path)\r",
	});
	const markerExpression =
		'new RegExp(' +
		JSON.stringify(powershellMarkerPattern.source) +
		", 'i').test(window.__windowsSmokeOutput)";
	await waitFor(markerExpression);
	const output = await evaluate('window.__windowsSmokeOutput');
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
		if (eventId !== undefined)
			await cleanupInvoke('plugin:event|unlisten', { event: 'terminal:data', eventId });
		if (terminalId && projectId) {
			try {
				await invoke('terminal_kill', { id: terminalId });
			} catch {
				await cleanupInvoke('shell_kill_for_project', { projectId });
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
