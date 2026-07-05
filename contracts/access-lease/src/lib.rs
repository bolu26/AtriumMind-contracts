#![no_std]
//! AtriumMind — Access Lease Contract (Soroban)
//!
//! Issues time-limited on-chain access grants to vault resources.
//! Any third party can verify a buyer's access via `is_valid` without
//! trusting the AtriumMind backend — the ledger is the source of truth.
//!
//! ## Auth model
//! - `init(admin)` — set once at deploy
//! - `grant/extend/revoke` — admin only (backend wallet)
//! - `is_valid/get_lease` — permissionless reads

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, String};

const DAY: u32 = 17_280;      // ~5s/ledger
const BUMP: u32 = 90 * DAY;
const BUMP_THRESH: u32 = BUMP - DAY;

#[contracttype] #[derive(Clone, Debug, PartialEq)]
pub struct Lease {
    pub resource_id:      String,
    pub buyer:            Address,
    pub granted_at:       u32,
    pub expires_at:       u32,
    pub duration_ledgers: u32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Lease(String, Address),
}

#[contracterror] #[derive(Copy, Clone, Debug, Eq, PartialEq)] #[repr(u32)]
pub enum Error {
    NotAdmin        = 1,
    LeaseNotFound   = 2,
    AlreadyActive   = 3,
    InvalidDuration = 4,
    NotInitialised  = 5,
}

#[contract] pub struct AccessLease;

#[contractimpl]
impl AccessLease {
    /// Deploy: set the admin address (backend platform wallet).
    pub fn init(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().bump(BUMP, BUMP_THRESH);
    }

    /// Grant a timed lease to `buyer` for `resource_id` (admin only).
    pub fn grant_lease(
        env: Env,
        resource_id: String,
        buyer: Address,
        duration_ledgers: u32,
    ) -> Result<Lease, Error> {
        Self::require_admin(&env)?;
        if duration_ledgers == 0 { return Err(Error::InvalidDuration); }

        let key = DataKey::Lease(resource_id.clone(), buyer.clone());
        if let Some(existing) = env.storage().persistent().get::<_, Lease>(&key) {
            if existing.expires_at > env.ledger().sequence() {
                return Err(Error::AlreadyActive);
            }
        }

        let now   = env.ledger().sequence();
        let lease = Lease {
            resource_id, buyer,
            granted_at:       now,
            expires_at:       now + duration_ledgers,
            duration_ledgers,
        };
        env.storage().persistent().set(&key, &lease);
        env.storage().persistent().bump(&key, BUMP, BUMP_THRESH);
        Ok(lease)
    }

    /// Extend an active or expired lease by `extra_ledgers` (admin only).
    pub fn extend_lease(
        env: Env, resource_id: String, buyer: Address, extra_ledgers: u32,
    ) -> Result<Lease, Error> {
        Self::require_admin(&env)?;
        let key = DataKey::Lease(resource_id.clone(), buyer.clone());
        let mut lease: Lease = env.storage().persistent().get(&key).ok_or(Error::LeaseNotFound)?;
        let base = lease.expires_at.max(env.ledger().sequence());
        lease.expires_at = base + extra_ledgers;
        env.storage().persistent().set(&key, &lease);
        env.storage().persistent().bump(&key, BUMP, BUMP_THRESH);
        Ok(lease)
    }

    /// Returns `true` if buyer holds a currently valid lease (permissionless).
    pub fn is_valid(env: Env, resource_id: String, buyer: Address) -> bool {
        match env.storage().persistent().get::<_, Lease>(&DataKey::Lease(resource_id, buyer)) {
            Some(l) => l.expires_at > env.ledger().sequence(),
            None    => false,
        }
    }

    /// Return the full lease struct (permissionless).
    pub fn get_lease(env: Env, resource_id: String, buyer: Address) -> Result<Lease, Error> {
        env.storage().persistent()
            .get(&DataKey::Lease(resource_id, buyer))
            .ok_or(Error::LeaseNotFound)
    }

    /// Revoke a lease — used on refund or ToS violation (admin only).
    pub fn revoke_lease(env: Env, resource_id: String, buyer: Address) -> Result<(), Error> {
        Self::require_admin(&env)?;
        let key = DataKey::Lease(resource_id, buyer);
        if !env.storage().persistent().has(&key) { return Err(Error::LeaseNotFound); }
        env.storage().persistent().remove(&key);
        Ok(())
    }

    // ── Internal ─────────────────────────────────────────────────────────
    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env.storage().instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialised)?;
        admin.require_auth();
        Ok(())
    }
}
