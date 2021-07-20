use borsh::{BorshDeserialize, BorshSerialize};

type Timestamp = solana_program::clock::UnixTimestamp;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AmpFactor {
    initial_value: u64,
    initial_timestamp: Timestamp,
    target_value: u64,
    target_timestamp: Timestamp,
}

impl AmpFactor {
    pub fn new(amp_factor: u64, ts: Timestamp) -> AmpFactor {
        AmpFactor{
            initial_value: amp_factor,
            initial_timestamp: ts,
            target_value: 0,
            target_timestamp: 0,
        }
    }

    pub fn get(&self, ts: Timestamp) {
        todo!("impl");
    }

    pub fn adjust(&mut self, value: u64, ts: Timestamp) {
        todo!("impl");
    }

    pub fn stop_adjustment(&mut self) {
        todo!("impl");
    }
}
