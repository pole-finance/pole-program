import { struct } from 'buffer-layout';
import { bool, u64 } from '../layout';

export interface LastUpdate {
    slot: bigint;
    stale: boolean;
}

/** @internal */
export const LastUpdateLayout = struct<LastUpdate>([u64('slot'), bool('stale')], 'lastUpdate');