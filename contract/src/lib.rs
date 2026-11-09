//! Leash: on-chain spending guardrails for an autonomous agent.
//!
//! The owner sets a per-transaction cap and a rolling daily cap. The agent
//! account can only move funds through `spend`, which reverts when a payment
//! would exceed either cap. The agent holds a NEAR function-call access key
//! scoped to this contract's `spend` method, so even a leaked agent key cannot
//! change the caps, withdraw funds, or attach a deposit.

use near_sdk::json_types::U128;
use near_sdk::serde::Serialize;
use near_sdk::{env, near, AccountId, NearToken, PanicOnDefault, Promise};

/// 24 hours in nanoseconds (NEAR block timestamps are in nanoseconds).
pub const WINDOW_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

pub const EVENT_STANDARD: &str = "leash";
pub const EVENT_VERSION: &str = "1.0.0";

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Leash {
    /// Account that sets caps and withdraws. Full-access key holder.
    owner: AccountId,
    /// Account allowed to call `spend`. Holds a function-call key only.
    agent: AccountId,
    /// Maximum yoctoNEAR the agent may spend in one call.
    per_tx_cap: u128,
    /// Maximum yoctoNEAR the agent may spend within one 24h window.
    daily_cap: u128,
    /// yoctoNEAR spent so far in the current window.
    spent_in_window: u128,
    /// Start of the current window in nanoseconds (0 until the first spend).
    window_start: u64,
    /// Count of successful spends.
    nonce: u64,
}

/// Read-only view of contract state. Caps and totals are yoctoNEAR strings.
#[near(serializers = [json])]
pub struct Config {
    pub owner: AccountId,
    pub agent: AccountId,
    pub per_tx_cap: U128,
    pub daily_cap: U128,
    pub spent_in_window: U128,
    pub window_start: u64,
    pub nonce: u64,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
struct SpendEventData<'a> {
    agent: &'a AccountId,
    to: &'a AccountId,
    amount: U128,
    spent_in_window: U128,
    nonce: u64,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
struct Event<'a, T: Serialize> {
    standard: &'a str,
    version: &'a str,
    event: &'a str,
    data: T,
}

fn emit_spend_event(data: SpendEventData) {
    let event = Event {
        standard: EVENT_STANDARD,
        version: EVENT_VERSION,
        event: "spend",
        data,
    };
    let body = near_sdk::serde_json::to_string(&event).expect("event serialization");
    env::log_str(&format!("EVENT_JSON:{}", body));
}

#[near]
impl Leash {
    /// Deploy-time initializer. Caps must be positive and per_tx_cap <= daily_cap.
    #[init]
    pub fn new(owner: AccountId, agent: AccountId, per_tx_cap: U128, daily_cap: U128) -> Self {
        assert!(owner != agent, "owner and agent must differ");
        validate_caps(per_tx_cap.0, daily_cap.0);
        Self {
            owner,
            agent,
            per_tx_cap: per_tx_cap.0,
            daily_cap: daily_cap.0,
            spent_in_window: 0,
            window_start: 0,
            nonce: 0,
        }
    }

    /// Owner-only. Requires exactly 1 yoctoNEAR attached, which a function-call
    /// key cannot provide, so a leaked agent key cannot change caps.
    #[payable]
    pub fn set_caps(&mut self, per_tx_cap: U128, daily_cap: U128) {
        assert_one_yocto();
        self.assert_owner();
        validate_caps(per_tx_cap.0, daily_cap.0);
        self.per_tx_cap = per_tx_cap.0;
        self.daily_cap = daily_cap.0;
    }

    /// Owner-only. Sends `amount` yoctoNEAR from the contract to the owner.
    /// Requires exactly 1 yoctoNEAR attached.
    #[payable]
    pub fn withdraw(&mut self, amount: U128) -> Promise {
        assert_one_yocto();
        self.assert_owner();
        assert!(amount.0 > 0, "amount must be positive");
        Promise::new(self.owner.clone()).transfer(NearToken::from_yoctonear(amount.0))
    }

    /// Agent-only. Transfers `amount` yoctoNEAR to `to`, reverting if the amount
    /// exceeds the per-transaction cap or would push the current 24h window
    /// over the daily cap. The window starts at the first spend and resets
    /// once 24 hours have passed since it started.
    pub fn spend(&mut self, to: AccountId, amount: U128) -> Promise {
        self.assert_agent();
        let amount = amount.0;
        assert!(amount > 0, "amount must be positive");
        assert!(amount <= self.per_tx_cap, "per-tx cap exceeded");

        let now = env::block_timestamp();
        if now.saturating_sub(self.window_start) >= WINDOW_NS {
            self.window_start = now;
            self.spent_in_window = 0;
        }

        let next_total = self
            .spent_in_window
            .checked_add(amount)
            .expect("spend overflow");
        assert!(next_total <= self.daily_cap, "daily cap exceeded");

        self.spent_in_window = next_total;
        self.nonce += 1;

        emit_spend_event(SpendEventData {
            agent: &self.agent,
            to: &to,
            amount: U128(amount),
            spent_in_window: U128(self.spent_in_window),
            nonce: self.nonce,
        });

        Promise::new(to).transfer(NearToken::from_yoctonear(amount))
    }

    /// Read-only view of the live guardrail state.
    pub fn get_config(&self) -> Config {
        Config {
            owner: self.owner.clone(),
            agent: self.agent.clone(),
            per_tx_cap: U128(self.per_tx_cap),
            daily_cap: U128(self.daily_cap),
            spent_in_window: U128(self.spent_in_window),
            window_start: self.window_start,
            nonce: self.nonce,
        }
    }
}

impl Leash {
    fn assert_owner(&self) {
        assert!(
            env::predecessor_account_id() == self.owner,
            "only owner may call this"
        );
    }

    fn assert_agent(&self) {
        assert!(
            env::predecessor_account_id() == self.agent,
            "only agent may call this"
        );
    }
}

fn assert_one_yocto() {
    assert_eq!(
        env::attached_deposit(),
        NearToken::from_yoctonear(1),
        "requires exactly 1 yoctoNEAR attached"
    );
}

fn validate_caps(per_tx_cap: u128, daily_cap: u128) {
    assert!(per_tx_cap > 0, "per-tx cap must be positive");
    assert!(daily_cap > 0, "daily cap must be positive");
    assert!(
        per_tx_cap <= daily_cap,
        "per-tx cap must not exceed daily cap"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
    const OWNER: &str = "leash-owner-0445ff.testnet";
    const AGENT: &str = "leash-agent-0445ff.testnet";
    const STRANGER: &str = "stranger.testnet";
    const RECIPIENT: &str = "merchant.testnet";
    const T0: u64 = 1_800_000_000_000_000_000;

    fn ctx(predecessor: &str, deposit: u128, ts: u64) -> VMContextBuilder {
        let mut b = VMContextBuilder::new();
        b.current_account_id("leash-0445ff.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .signer_account_id(predecessor.parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_timestamp(ts)
            .account_balance(NearToken::from_near(100));
        b
    }

    fn new_leash(per_tx: u128, daily: u128) -> Leash {
        testing_env!(ctx(OWNER, 0, T0).build());
        Leash::new(
            OWNER.parse().unwrap(),
            AGENT.parse().unwrap(),
            U128(per_tx),
            U128(daily),
        )
    }

    fn agent_spend(leash: &mut Leash, amount: u128, ts: u64) {
        testing_env!(ctx(AGENT, 0, ts).build());
        leash.spend(RECIPIENT.parse().unwrap(), U128(amount));
    }

    #[test]
    fn approved_spend_passes() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        agent_spend(&mut leash, 3 * NEAR, T0);
        let cfg = leash.get_config();
        assert_eq!(cfg.spent_in_window.0, 3 * NEAR);
        assert_eq!(cfg.nonce, 1);
        assert_eq!(cfg.window_start, T0);
    }

    #[test]
    #[should_panic(expected = "per-tx cap exceeded")]
    fn per_tx_over_cap_reverts() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        agent_spend(&mut leash, 4 * NEAR + 1, T0);
    }

    #[test]
    #[should_panic(expected = "daily cap exceeded")]
    fn daily_cap_reverts() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        agent_spend(&mut leash, 4 * NEAR, T0);
        agent_spend(&mut leash, 4 * NEAR, T0 + 1);
        agent_spend(&mut leash, 1, T0 + 2);
    }

    #[test]
    fn window_resets_after_24h() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        agent_spend(&mut leash, 4 * NEAR, T0);
        agent_spend(&mut leash, 4 * NEAR, T0 + 1);
        agent_spend(&mut leash, 4 * NEAR, T0 + WINDOW_NS);
        let cfg = leash.get_config();
        assert_eq!(cfg.spent_in_window.0, 4 * NEAR);
        assert_eq!(cfg.window_start, T0 + WINDOW_NS);
        assert_eq!(cfg.nonce, 3);
    }

    #[test]
    #[should_panic(expected = "daily cap exceeded")]
    fn still_blocked_inside_window() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        agent_spend(&mut leash, 4 * NEAR, T0);
        agent_spend(&mut leash, 4 * NEAR, T0 + WINDOW_NS - 1);
        agent_spend(&mut leash, 4 * NEAR, T0 + WINDOW_NS - 1);
    }

    #[test]
    #[should_panic(expected = "only agent may call this")]
    fn stranger_rejected() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(STRANGER, 0, T0).build());
        leash.spend(RECIPIENT.parse().unwrap(), U128(NEAR));
    }

    #[test]
    fn owner_can_raise_caps() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(OWNER, 1, T0).build());
        leash.set_caps(U128(10 * NEAR), U128(20 * NEAR));
        let cfg = leash.get_config();
        assert_eq!(cfg.per_tx_cap.0, 10 * NEAR);
        assert_eq!(cfg.daily_cap.0, 20 * NEAR);
    }

    #[test]
    #[should_panic(expected = "only owner may call this")]
    fn agent_cannot_raise_caps() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(AGENT, 1, T0).build());
        leash.set_caps(U128(10 * NEAR), U128(20 * NEAR));
    }

    #[test]
    #[should_panic(expected = "requires exactly 1 yoctoNEAR attached")]
    fn missing_one_yocto_reverts() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(OWNER, 0, T0).build());
        leash.set_caps(U128(10 * NEAR), U128(20 * NEAR));
    }

    #[test]
    #[should_panic(expected = "per-tx cap must not exceed daily cap")]
    fn invalid_caps_rejected() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(OWNER, 1, T0).build());
        leash.set_caps(U128(9 * NEAR), U128(8 * NEAR));
    }

    #[test]
    #[should_panic(expected = "per-tx cap must be positive")]
    fn zero_cap_rejected() {
        let mut leash = new_leash(4 * NEAR, 8 * NEAR);
        testing_env!(ctx(OWNER, 1, T0).build());
        leash.set_caps(U128(0), U128(8 * NEAR));
    }
}
