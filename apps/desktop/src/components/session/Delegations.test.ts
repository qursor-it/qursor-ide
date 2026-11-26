import { describeDelegation } from '@components/session/Delegations';
import { describe, expect, it } from 'vitest';

describe('describeDelegation', () => {
	it('waits on the human before it starts', () => {
		expect(describeDelegation('pendingApproval', undefined).label).toBe('Awaiting your approval');
	});

	it('reads as starting until its PTY reports', () => {
		expect(describeDelegation('starting', 'working').label).toBe('Starting');
		expect(describeDelegation('running', undefined).label).toBe('Starting');
	});

	it('takes the live PTY state once running', () => {
		expect(describeDelegation('running', 'working')).toMatchObject({
			label: 'Working',
			dot: 'working',
		});
		expect(describeDelegation('running', 'waiting_input').label).toBe('Needs you');
		expect(describeDelegation('running', 'unknown')).toMatchObject({
			label: 'Running',
			dot: 'unknown',
		});
	});
});
