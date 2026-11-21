import type { Server, Tunnel } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
	Input,
	Label,
	Switch,
	toast,
} from '@factorai/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { formatError } from '@lib/errors';
import { cmd } from '@lib/tauri';

const describe = (t: Tunnel) =>
	`${t.label ? `${t.label} · ` : ''}localhost:${t.localPort} → ${t.destHost}:${t.destPort}`;

/**
 * A server's local port forwards (F48): each a switch, listening on
 * 127.0.0.1 only, carried over the server's login.
 */
export function TunnelsMenu({ server }: { server: Server }) {
	const qc = useQueryClient();
	const key = ['tunnels', server.id] as const;
	const list = useQuery({ queryKey: key, queryFn: () => cmd.listTunnels(server.id) });
	const [draft, setDraft] = useState<{
		label: string;
		local: string;
		host: string;
		dest: string;
	} | null>(null);
	const refresh = () => qc.invalidateQueries({ queryKey: key });
	const running = list.data?.filter((t) => t.running).length ?? 0;

	const toggle = (t: Tunnel, on: boolean) =>
		(on ? cmd.tunnelStart(t.id) : cmd.tunnelStop(t.id))
			.catch((e) => toast.error(`Tunnel ${describe(t)}: ${formatError(e)}`))
			.finally(refresh);
	const save = () => {
		if (!draft) return;
		cmd
			.saveTunnel({
				id: '',
				serverId: server.id,
				label: draft.label,
				localPort: Number.parseInt(draft.local, 10) || 0,
				destHost: draft.host,
				destPort: Number.parseInt(draft.dest, 10) || 0,
				running: false,
			})
			.then(() => {
				setDraft(null);
				refresh();
			})
			.catch((e) => toast.error(formatError(e)));
	};

	return (
		<>
			<DropdownMenu>
				<DropdownMenuTrigger asChild>
					<Button size="sm" variant="ghost" data-testid="tunnels-menu">
						Tunnels{running > 0 ? ` · ${running}` : ''}
					</Button>
				</DropdownMenuTrigger>
				<DropdownMenuContent align="end" className="w-80">
					<DropdownMenuLabel>Local port forwards on {server.name}</DropdownMenuLabel>
					{(list.data ?? []).map((t) => (
						<div
							key={t.id}
							className="flex h-8 items-center gap-2 px-2 text-sm"
							data-testid="tunnel-row"
						>
							<Switch
								aria-label={describe(t)}
								checked={t.running}
								onCheckedChange={(on) => toggle(t, on)}
							/>
							<span className="min-w-0 flex-1 truncate font-mono text-xs">{describe(t)}</span>
						</div>
					))}
					{(list.data ?? []).length > 0 && <DropdownMenuSeparator />}
					<DropdownMenuItem
						onSelect={() => setDraft({ label: '', local: '', host: 'localhost', dest: '' })}
					>
						Add tunnel…
					</DropdownMenuItem>
					{(list.data ?? []).map((t) => (
						<DropdownMenuItem
							key={`del-${t.id}`}
							className="text-destructive"
							onSelect={() => cmd.deleteTunnel(t.id).then(refresh)}
						>
							Remove {describe(t)}
						</DropdownMenuItem>
					))}
				</DropdownMenuContent>
			</DropdownMenu>
			<Dialog open={draft !== null} onOpenChange={(o) => !o && setDraft(null)}>
				<DialogContent data-testid="tunnel-dialog">
					<DialogHeader>
						<DialogTitle>New tunnel on {server.name}</DialogTitle>
						<DialogDescription>
							A port on this computer (127.0.0.1 only) that reaches a host and port as {server.name}{' '}
							sees them — like ssh -L.
						</DialogDescription>
					</DialogHeader>
					<div className="grid grid-cols-[8rem_1fr] items-center gap-2.5 text-sm">
						<Label htmlFor="tun-local">Local port</Label>
						<Input
							id="tun-local"
							inputMode="numeric"
							placeholder="5433"
							value={draft?.local ?? ''}
							onChange={(e) =>
								setDraft((d) => d && { ...d, local: e.target.value.replace(/\D/g, '') })
							}
						/>
						<Label htmlFor="tun-host">Destination</Label>
						<div className="flex gap-2">
							<Input
								id="tun-host"
								value={draft?.host ?? ''}
								onChange={(e) => setDraft((d) => d && { ...d, host: e.target.value })}
							/>
							<Input
								aria-label="Destination port"
								className="w-24"
								inputMode="numeric"
								placeholder="5432"
								value={draft?.dest ?? ''}
								onChange={(e) =>
									setDraft((d) => d && { ...d, dest: e.target.value.replace(/\D/g, '') })
								}
							/>
						</div>
						<Label htmlFor="tun-label">Label</Label>
						<Input
							id="tun-label"
							placeholder="Postgres (optional)"
							value={draft?.label ?? ''}
							onChange={(e) => setDraft((d) => d && { ...d, label: e.target.value })}
						/>
					</div>
					<DialogFooter>
						<Button variant="outline" onClick={() => setDraft(null)}>
							Cancel
						</Button>
						<Button
							data-testid="tunnel-save"
							disabled={!draft?.local || !draft.dest || !draft.host.trim()}
							onClick={save}
						>
							Add tunnel
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</>
	);
}
