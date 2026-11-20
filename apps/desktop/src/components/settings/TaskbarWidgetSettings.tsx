import type { TaskbarWidgetConfig } from '@factorai/types';
import {
	Button,
	IconButton,
	Input,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	SettingRow,
	Switch,
} from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { ArrowDown, ArrowUp } from 'lucide-react';
import { agentName } from '@lib/agents';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import {
	FONT_PX_RANGE,
	hasDefaultGeometry,
	moveAgent,
	profilesFor,
	withDefaultGeometry,
} from '@lib/taskbarWidget';

interface TaskbarWidgetSettingsProps {
	value: TaskbarWidgetConfig;
	onChange: (value: TaskbarWidgetConfig) => void;
}

const SHOW_SWITCHES: readonly {
	key: keyof TaskbarWidgetConfig['show'];
	label: string;
}[] = [
	{ key: 'fiveHourBar', label: '5-hour bar' },
	{ key: 'fiveHourPercent', label: '5-hour percentage' },
	{ key: 'fiveHourReset', label: '5-hour reset countdown' },
	{ key: 'weeklyBar', label: 'Weekly bar' },
	{ key: 'weeklyPercent', label: 'Weekly percentage' },
	{ key: 'weeklyReset', label: 'Weekly reset countdown' },
];

/** The value used for "the agent's default profile" in the profile picker: a
 *  select item cannot carry an empty value. */
const DEFAULT_PROFILE = '__default__';

/**
 * The taskbar widget's layout, under the usage section's widget switch. Edits
 * the draft like every other row: nothing reaches the widget until Save, which
 * writes it through `set_taskbar_widget_config`. The widget's position and size
 * are not fields here: they are dragged on the taskbar itself, and this form
 * only offers to put them back.
 */
export function TaskbarWidgetSettings({ value, onChange }: TaskbarWidgetSettingsProps) {
	const profiles = useQuery({
		queryKey: queryKeys.profiles(),
		queryFn: () => cmd.listProfiles(),
		retry: false,
	});
	const patch = (changes: Partial<TaskbarWidgetConfig>) => onChange({ ...value, ...changes });
	const setAgent = (index: number, changes: Partial<TaskbarWidgetConfig['agents'][number]>) =>
		patch({ agents: value.agents.map((a, i) => (i === index ? { ...a, ...changes } : a)) });
	const setColor = (changes: Partial<TaskbarWidgetConfig['colors']>) =>
		patch({ colors: { ...value.colors, ...changes } });

	return (
		<div data-testid="settings-widget" className="mt-1 space-y-1 border-border border-l pl-4">
			<h3 className="pt-2 font-medium text-muted-foreground text-xs uppercase tracking-wider">
				Taskbar widget
			</h3>
			<div className="divide-y divide-border">
				<SettingRow
					label="Show"
					stacked
					description="What each agent's line shows. When the widget is too narrow, the weekly countdown goes first, then the 5-hour one, the weekly bar, the weekly percentage and the 5-hour bar."
				>
					<div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
						{SHOW_SWITCHES.map(({ key, label }) => (
							<label key={key} className="flex items-center justify-between gap-2 text-xs">
								{label}
								<Switch
									data-testid={`settings-widget-show-${key}`}
									checked={value.show[key]}
									onCheckedChange={(v) => patch({ show: { ...value.show, [key]: v } })}
								/>
							</label>
						))}
					</div>
				</SettingRow>

				<SettingRow
					label="Agents"
					stacked
					description="Which agents the widget lists, in this order, and the profile each one reads. The default profile follows the one you set in Profiles."
				>
					<div className="space-y-1.5">
						{value.agents.map((slot, index) => {
							const options = profilesFor(profiles.data ?? [], slot.agent);
							// A profile that is gone shows as the default, as the widget draws it.
							const picked = options.some((p) => p.id === slot.profileId)
								? (slot.profileId ?? DEFAULT_PROFILE)
								: DEFAULT_PROFILE;
							return (
								<div
									key={slot.agent}
									data-testid={`settings-widget-agent-${slot.agent}`}
									className="flex items-center gap-2 text-xs"
								>
									<Switch
										aria-label={`Show ${agentName(slot.agent)}`}
										data-testid={`settings-widget-agent-${slot.agent}-visible`}
										checked={slot.visible}
										onCheckedChange={(v) => setAgent(index, { visible: v })}
									/>
									<span className="w-20 shrink-0">{agentName(slot.agent)}</span>
									<Select
										value={picked}
										onValueChange={(v) =>
											setAgent(index, { profileId: v === DEFAULT_PROFILE ? null : v })
										}
									>
										<SelectTrigger
											aria-label={`${agentName(slot.agent)} profile`}
											data-testid={`settings-widget-agent-${slot.agent}-profile`}
											className="w-44"
										>
											<SelectValue />
										</SelectTrigger>
										<SelectContent>
											<SelectItem value={DEFAULT_PROFILE}>Default profile</SelectItem>
											{options.map((p) => (
												<SelectItem key={p.id} value={p.id}>
													{p.name}
												</SelectItem>
											))}
										</SelectContent>
									</Select>
									<IconButton
										size="md"
										aria-label={`Move ${agentName(slot.agent)} up`}
										title="Move up"
										disabled={index === 0}
										onClick={() => patch({ agents: moveAgent(value.agents, index, -1) })}
									>
										<ArrowUp />
									</IconButton>
									<IconButton
										size="md"
										aria-label={`Move ${agentName(slot.agent)} down`}
										title="Move down"
										disabled={index === value.agents.length - 1}
										onClick={() => patch({ agents: moveAgent(value.agents, index, 1) })}
									>
										<ArrowDown />
									</IconButton>
								</div>
							);
						})}
					</div>
				</SettingRow>

				<ChoiceRow
					id="layout"
					label="Layout"
					description="One row per agent, or every agent on one row."
					value={value.layout}
					options={[
						['rows', 'Rows'],
						['inline', 'Inline'],
					]}
					onChange={(layout) => patch({ layout })}
				/>
				<ChoiceRow
					id="labels"
					label="Agent labels"
					value={value.labels}
					options={[
						['letter', 'C / X'],
						['name', 'Claude / Codex'],
					]}
					onChange={(labels) => patch({ labels })}
				/>
				<SettingRow
					label="Font size"
					htmlFor="settings-widget-font"
					description="In pixels. Rows grow with it, up to the taskbar's height."
				>
					<Input
						id="settings-widget-font"
						data-testid="settings-widget-font"
						className="w-16"
						type="number"
						min={FONT_PX_RANGE.min}
						max={FONT_PX_RANGE.max}
						value={value.fontPx}
						onChange={(e) => {
							const n = Number(e.target.value);
							if (Number.isFinite(n) && e.target.value !== '') patch({ fontPx: n });
						}}
					/>
				</SettingRow>
				<ChoiceRow
					id="theme"
					label="Text colour"
					description="Follow the system, or say which taskbar the text is drawn for."
					value={value.theme}
					options={[
						['auto', 'Follow system'],
						['light', 'Light taskbar (dark text)'],
						['dark', 'Dark taskbar (light text)'],
					]}
					onChange={(theme) => patch({ theme })}
				/>
				<SettingRow
					label="Background"
					htmlFor="settings-widget-background"
					description="A rounded backing in the opposite colour of the text, from 0 (none) to 100%."
				>
					<div className="flex items-center gap-1.5">
						<Input
							id="settings-widget-background"
							data-testid="settings-widget-background"
							className="w-16"
							type="number"
							min={0}
							max={100}
							value={Math.round(value.backgroundOpacity * 100)}
							onChange={(e) => {
								const n = Number(e.target.value);
								if (Number.isFinite(n) && e.target.value !== '')
									patch({ backgroundOpacity: Math.min(100, Math.max(0, n)) / 100 });
							}}
						/>
						<span className="text-muted-foreground text-xs">%</span>
					</div>
				</SettingRow>
				<SettingRow
					label="Bar colours"
					stacked
					description="Warning and critical colours apply to the widget only; the IDE keeps its own. The normal colour is the text colour, a little dimmed, unless you pick one."
				>
					<div className="flex flex-wrap items-center gap-4 text-xs">
						<ColorField
							id="warn"
							label="Warning"
							value={value.colors.warn}
							onChange={(warn) => setColor({ warn })}
						/>
						<ColorField
							id="critical"
							label="Critical"
							value={value.colors.critical}
							onChange={(critical) => setColor({ critical })}
						/>
						<div className="flex items-center gap-1.5">
							{value.colors.normal === null ? (
								<Button
									variant="outline"
									size="sm"
									data-testid="settings-widget-color-normal-custom"
									onClick={() => setColor({ normal: '#9CA3AF' })}
								>
									Normal: default
								</Button>
							) : (
								<>
									<ColorField
										id="normal"
										label="Normal"
										value={value.colors.normal}
										onChange={(normal) => setColor({ normal })}
									/>
									<Button
										variant="outline"
										size="sm"
										data-testid="settings-widget-color-normal-default"
										onClick={() => setColor({ normal: null })}
									>
										Use default
									</Button>
								</>
							)}
						</div>
					</div>
				</SettingRow>
				<ChoiceRow
					id="click"
					label="Click"
					description="What a click on the widget does. Drag it to move it; drag its left or right edge to resize it."
					value={value.click}
					options={[
						['focus', 'Open Qursor IDE'],
						['details', 'Open usage details'],
						['none', 'Nothing'],
					]}
					onChange={(click) => patch({ click })}
				/>
				<SettingRow
					label="Position and size"
					description="Put the widget back beside the system tray at its original width."
				>
					<Button
						variant="outline"
						size="sm"
						data-testid="settings-widget-reset"
						disabled={hasDefaultGeometry(value)}
						onClick={() => onChange(withDefaultGeometry(value))}
					>
						Reset position and size
					</Button>
				</SettingRow>
			</div>
		</div>
	);
}

function ChoiceRow<T extends string>({
	id,
	label,
	description,
	value,
	options,
	onChange,
}: {
	id: string;
	label: string;
	description?: string;
	value: T;
	options: readonly (readonly [T, string])[];
	onChange: (value: T) => void;
}) {
	return (
		<SettingRow label={label} htmlFor={`settings-widget-${id}`} description={description}>
			<Select value={value} onValueChange={(v) => onChange(v as T)}>
				<SelectTrigger
					id={`settings-widget-${id}`}
					data-testid={`settings-widget-${id}`}
					className="w-52"
				>
					<SelectValue />
				</SelectTrigger>
				<SelectContent>
					{options.map(([option, text]) => (
						<SelectItem key={option} value={option} data-testid={`settings-widget-${id}-${option}`}>
							{text}
						</SelectItem>
					))}
				</SelectContent>
			</Select>
		</SettingRow>
	);
}

function ColorField({
	id,
	label,
	value,
	onChange,
}: {
	id: string;
	label: string;
	value: string;
	onChange: (value: string) => void;
}) {
	return (
		<label className="flex items-center gap-1.5">
			{label}
			<Input
				type="color"
				data-testid={`settings-widget-color-${id}`}
				className="h-7 w-10 p-0.5"
				value={value}
				onChange={(e) => onChange(e.target.value.toUpperCase())}
			/>
		</label>
	);
}
