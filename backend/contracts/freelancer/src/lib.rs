#![no_std]

extern crate alloc;
use alloc::format;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Symbol};

/// Freelancer Profile
#[contracttype]
pub struct FreelancerProfile {
    pub address: Address,
    pub name: String,
    pub discipline: String,
    pub bio: String,
    pub rating: u32, // 0-500 represents 0-5 stars (fixed point)
    pub total_rating_count: u32,
    pub completed_projects: u32,
    pub total_earnings: i128,
    pub verified: bool,
    pub created_at: u64,
}

/// Storage key for the contract owner address (set once at init).
const OWNER_KEY: &str = "owner";

/// Storage key prefix for the public-key → user mapping.
const PK_MAP_PREFIX: &str = "pk_map_";

#[contract]
pub struct FreelancerContract;

#[contractimpl]
impl FreelancerContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Set the contract owner. Must be called once after deployment.
    pub fn initialize(env: Env, owner: Address) {
        owner.require_auth();
        let owner_key = Symbol::new(&env, OWNER_KEY);
        if env.storage().persistent().has(&owner_key) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&owner_key, &owner);
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn get_owner(env: &Env) -> Address {
        let owner_key = Symbol::new(env, OWNER_KEY);
        env.storage()
            .persistent()
            .get::<Symbol, Address>(&owner_key)
            .expect("Contract not initialized")
    }

    // ── Public-key → user mapping (owner-only) ────────────────────────────────

    /// Map a Stellar public key to a user address. Only the contract owner may call this.
    pub fn set_public_key_mapping(env: Env, public_key: String, user: Address) {
        // #313: owner-only guard
        let owner = Self::get_owner(&env);
        owner.require_auth();

        let map_key = Symbol::new(&env, &format!("{}{:?}", PK_MAP_PREFIX, public_key));
        env.storage().persistent().set(&map_key, &user);
    }

    /// Retrieve the user address associated with a public key.
    pub fn get_user_by_public_key(env: Env, public_key: String) -> Option<Address> {
        let map_key = Symbol::new(&env, &format!("{}{:?}", PK_MAP_PREFIX, public_key));
        env.storage().persistent().get::<Symbol, Address>(&map_key)
    }

    // ── Freelancer registration ───────────────────────────────────────────────

    pub fn register_freelancer(
        env: Env,
        freelancer: Address,
        name: String,
        discipline: String,
        bio: String,
    ) -> bool {
        // #312: caller must authorise this action
        freelancer.require_auth();

        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));

        if env.storage().persistent().has(&profile_key) {
            return false;
        }

        let profile = FreelancerProfile {
            address: freelancer.clone(),
            name,
            discipline,
            bio,
            rating: 0,
            total_rating_count: 0,
            completed_projects: 0,
            total_earnings: 0,
            verified: false,
            created_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&profile_key, &profile);

        let count_key = Symbol::new(&env, "freelancer_count");
        let count: u32 = env
            .storage()
            .persistent()
            .get::<Symbol, u32>(&count_key)
            .unwrap_or(0);
        env.storage().persistent().set(&count_key, &(count + 1));

        true
    }

    pub fn get_profile(env: Env, freelancer: Address) -> FreelancerProfile {
        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        env.storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
            .expect("Freelancer not registered")
    }

    /// Update the rating for a freelancer. Only the contract owner may call this
    /// to prevent self-rating abuse. (#312)
    pub fn update_rating(env: Env, freelancer: Address, new_rating: u32) -> bool {
        // #312: owner-only — ratings must come from the platform, not self
        let owner = Self::get_owner(&env);
        owner.require_auth();

        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        let mut profile = env
            .storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
            .expect("Freelancer not registered");

        let total = (profile.rating as u64) * (profile.total_rating_count as u64);
        let new_total = total + (new_rating as u64);
        profile.total_rating_count += 1;
        profile.rating = (new_total / (profile.total_rating_count as u64)) as u32;

        env.storage().persistent().set(&profile_key, &profile);
        true
    }

    /// Record a completed project. Only the contract owner may call this. (#312)
    pub fn update_completed_projects(env: Env, freelancer: Address) -> bool {
        // #312: owner-only — completion must be confirmed by the platform
        let owner = Self::get_owner(&env);
        owner.require_auth();

        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        let mut profile = env
            .storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
            .expect("Freelancer not registered");

        profile.completed_projects += 1;
        env.storage().persistent().set(&profile_key, &profile);
        true
    }

    /// Update earnings for a freelancer. Only the contract owner may call this. (#312)
    pub fn update_earnings(env: Env, freelancer: Address, amount: i128) -> bool {
        // #312: owner-only — earnings are credited by the escrow/platform
        let owner = Self::get_owner(&env);
        owner.require_auth();

        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        let mut profile = env
            .storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
            .expect("Freelancer not registered");

        profile.total_earnings += amount;
        env.storage().persistent().set(&profile_key, &profile);
        true
    }

    /// Verify a freelancer. Only the contract owner may call this. (#312)
    pub fn verify_freelancer(env: Env, freelancer: Address) -> bool {
        // #312: owner-only — verification is a privileged admin action
        let owner = Self::get_owner(&env);
        owner.require_auth();

        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        let mut profile = env
            .storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
            .expect("Freelancer not registered");

        profile.verified = true;
        env.storage().persistent().set(&profile_key, &profile);
        true
    }

    pub fn is_verified(env: Env, freelancer: Address) -> bool {
        let profile_key = Symbol::new(&env, &format!("profile_{:?}", freelancer));
        if let Some(profile) = env
            .storage()
            .persistent()
            .get::<Symbol, FreelancerProfile>(&profile_key)
        {
            profile.verified
        } else {
            false
        }
    }

    pub fn get_freelancers_count(env: Env) -> u32 {
        let count_key = Symbol::new(&env, "freelancer_count");
        env.storage()
            .persistent()
            .get::<Symbol, u32>(&count_key)
            .unwrap_or(0)
    }
}
