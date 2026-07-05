#![no_std]
//! AtriumMind — Subscription Manager (Soroban)
//!
//! Manages recurring 30-day subscription plans.
//! Publishers define plans; the backend subscribes buyers post-payment
//! and renews each billing cycle. Subscribers may self-cancel with
//! access through the end of the current period.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, String};

const DAY:   u32 = 17_280;
const CYCLE: u32 = 30 * DAY;
const BUMP:  u32 = 365 * DAY;
const BUMP_T: u32 = BUMP - DAY;

#[contracttype] #[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub plan_id:        String,
    pub publisher:      Address,
    pub price_per_cycle: i128,
    pub active:         bool,
}

#[contracttype] #[derive(Clone, Debug, PartialEq)]
pub struct Subscription {
    pub plan_id:             String,
    pub subscriber:          Address,
    pub started_at:          u32,
    pub current_period_end:  u32,
    pub cancelled:           bool,
    pub total_renewals:      u32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Plan(String),
    Sub(String, Address),
}

#[contracterror] #[derive(Copy, Clone, Debug, Eq, PartialEq)] #[repr(u32)]
pub enum Error {
    NotAdmin        = 1,
    PlanNotFound    = 2,
    PlanInactive    = 3,
    AlreadySubbed   = 4,
    SubNotFound     = 5,
    Cancelled       = 6,
    InvalidPrice    = 7,
    NotInitialised  = 8,
}

#[contract] pub struct SubscriptionManager;

#[contractimpl]
impl SubscriptionManager {
    pub fn init(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().bump(BUMP, BUMP_T);
    }

    /// Publisher creates a plan. `price_per_cycle` is in USDC stroops.
    pub fn create_plan(
        env: Env, publisher: Address, plan_id: String, price_per_cycle: i128,
    ) -> Result<Plan, Error> {
        publisher.require_auth();
        if price_per_cycle <= 0 { return Err(Error::InvalidPrice); }
        let plan = Plan { plan_id: plan_id.clone(), publisher, price_per_cycle, active: true };
        env.storage().persistent().set(&DataKey::Plan(plan_id.clone()), &plan);
        env.storage().persistent().bump(&DataKey::Plan(plan_id), BUMP, BUMP_T);
        Ok(plan)
    }

    /// Publisher deactivates a plan (no new subscribers).
    pub fn deactivate_plan(env: Env, plan_id: String) -> Result<(), Error> {
        let key = DataKey::Plan(plan_id.clone());
        let mut plan: Plan = env.storage().persistent().get(&key).ok_or(Error::PlanNotFound)?;
        plan.publisher.require_auth();
        plan.active = false;
        env.storage().persistent().set(&key, &plan);
        Ok(())
    }

    /// Backend subscribes a buyer after payment is confirmed (admin only).
    pub fn subscribe(env: Env, plan_id: String, subscriber: Address) -> Result<Subscription, Error> {
        Self::require_admin(&env)?;
        let plan: Plan = env.storage().persistent()
            .get(&DataKey::Plan(plan_id.clone()))
            .ok_or(Error::PlanNotFound)?;
        if !plan.active { return Err(Error::PlanInactive); }

        let key = DataKey::Sub(plan_id.clone(), subscriber.clone());
        if let Some(s) = env.storage().persistent().get::<_, Subscription>(&key) {
            if !s.cancelled && s.current_period_end > env.ledger().sequence() {
                return Err(Error::AlreadySubbed);
            }
        }

        let now = env.ledger().sequence();
        let sub = Subscription {
            plan_id, subscriber,
            started_at:         now,
            current_period_end: now + CYCLE,
            cancelled:          false,
            total_renewals:     0,
        };
        env.storage().persistent().set(&key, &sub);
        env.storage().persistent().bump(&key, BUMP, BUMP_T);
        Ok(sub)
    }

    /// Backend renews by one cycle (admin only).
    pub fn renew(env: Env, plan_id: String, subscriber: Address) -> Result<Subscription, Error> {
        Self::require_admin(&env)?;
        let key = DataKey::Sub(plan_id.clone(), subscriber.clone());
        let mut sub: Subscription = env.storage().persistent().get(&key).ok_or(Error::SubNotFound)?;
        if sub.cancelled { return Err(Error::Cancelled); }
        let base = sub.current_period_end.max(env.ledger().sequence());
        sub.current_period_end = base + CYCLE;
        sub.total_renewals += 1;
        env.storage().persistent().set(&key, &sub);
        env.storage().persistent().bump(&key, BUMP, BUMP_T);
        Ok(sub)
    }

    /// Subscriber cancels. Access persists until `current_period_end`.
    pub fn cancel(env: Env, plan_id: String, subscriber: Address) -> Result<(), Error> {
        subscriber.require_auth();
        let key = DataKey::Sub(plan_id, subscriber);
        let mut sub: Subscription = env.storage().persistent().get(&key).ok_or(Error::SubNotFound)?;
        sub.cancelled = true;
        env.storage().persistent().set(&key, &sub);
        Ok(())
    }

    /// Returns `true` if subscriber has active, non-expired access.
    pub fn is_active(env: Env, plan_id: String, subscriber: Address) -> bool {
        match env.storage().persistent().get::<_, Subscription>(&DataKey::Sub(plan_id, subscriber)) {
            Some(s) => !s.cancelled && s.current_period_end > env.ledger().sequence(),
            None    => false,
        }
    }

    pub fn get_subscription(env: Env, plan_id: String, subscriber: Address) -> Result<Subscription, Error> {
        env.storage().persistent()
            .get(&DataKey::Sub(plan_id, subscriber))
            .ok_or(Error::SubNotFound)
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env.storage().instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialised)?;
        admin.require_auth();
        Ok(())
    }
}
