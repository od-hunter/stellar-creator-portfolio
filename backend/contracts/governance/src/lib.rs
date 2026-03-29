To resolve these conflicts, I have merged the **Weighted Voting** features (Reputation and Tokens) from my branch with the **Fee & Parameter** updates from the `main` branch. 

**This is the complete, multi-featured Governance contract. You can copy and paste this entire file:**

```rust
#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, IntoVal, Symbol, Vec};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    Owner,
    Admin(Address),
    AdminList,
    Delegate(Address),
    ProposalCounter,
    Proposal(u64),
    HasVoted(u64, Address),
    FreelancerContract, // Link to freelancer contract for reputation weights
    TokenContract,      // Link to platform token contract for token-based voting
    PlatformFee,        // From main: platform fee state
    Parameter(Symbol),  // From main: dynamic parameters
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FreelancerProfile {
    pub address: Address,
    pub name: soroban_sdk::String,
    pub discipline: soroban_sdk::String,
    pub bio: soroban_sdk::String,
    pub rating: u32,
    pub total_rating_count: u32,
    pub completed_projects: u32,
    pub total_earnings: i128,
    pub verified: bool,
    pub created_at: u64,
    pub skills: soroban_sdk::Vec<soroban_sdk::String>,
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalType {
    AddAdmin(Address),
    RemoveAdmin(Address),
    FeeChange(u32),
    ParameterUpdate(Symbol, u32),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Pending = 0,
    Executed = 1,
    Rejected = 2,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u64,
    pub creator: Address,
    pub prop_type: ProposalType,
    pub status: ProposalStatus,
    pub votes_for: i128,    // Weighted voting uses i128
    pub votes_against: i128, 
    pub created_at: u64,
}

// ── Events ───────────────────────────────────────────────────────────────────

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct AdminAddedEvent {
    pub admin: Address,
    pub added_by: Address,
    pub timestamp: u64,
}

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct AdminRemovedEvent {
    pub admin: Address,
    pub removed_by: Address,
    pub timestamp: u64,
}

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalCreatedEvent {
    pub proposal_id: u64,
    pub proposer: Address,
    pub timestamp: u64,
}

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct VoteCastEvent {
    pub proposal_id: u64,
    pub voter: Address,
    pub support: bool,
    pub weight: i128, // Using weighted power
    pub timestamp: u64,
}

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalExecutedEvent {
    pub proposal_id: u64,
    pub executor: Address,
    pub status: u32,
    pub votes_for: i128,
    pub votes_against: i128,
    pub timestamp: u64,
}

const GOV: Symbol = symbol_short!("gov");

// ── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract owner.
    pub fn init(env: Env, owner: Address) -> bool {
        owner.require_auth();
        if env.storage().persistent().has(&DataKey::Owner) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&DataKey::Owner, &owner);
        env.storage()
            .persistent()
            .set(&DataKey::AdminList, &Vec::<Address>::new(&env));
        true
    }

    /// Set the freelancer contract to query reputation/earnings.
    pub fn set_freelancer_contract(env: Env, owner: Address, freelancer_contract: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env.storage().persistent().get(&DataKey::Owner).expect("Not initialized");
        if owner != stored_owner {
            panic!("Only owner can set freelancer contract");
        }
        env.storage().persistent().set(&DataKey::FreelancerContract, &freelancer_contract);
        true
    }

    /// Set the platform token contract for token-based voting weights.
    pub fn set_token_contract(env: Env, owner: Address, token_contract: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env.storage().persistent().get(&DataKey::Owner).expect("Not initialized");
        if owner != stored_owner {
            panic!("Only owner can set token contract");
        }
        env.storage().persistent().set(&DataKey::TokenContract, &token_contract);
        true
    }

    pub fn add_admin(env: Env, owner: Address, admin: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env.storage().persistent().get(&DataKey::Owner).expect("Not initialized");
        if stored_owner != owner { panic!("Unauthorized"); }

        Self::add_admin_internal(env.clone(), admin.clone());
        env.events().publish(
            (GOV, symbol_short!("adm_add")),
            AdminAddedEvent { admin, added_by: owner, timestamp: env.ledger().timestamp() },
        );
        true
    }

    pub fn remove_admin(env: Env, owner: Address, admin: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env.storage().persistent().get(&DataKey::Owner).expect("Not initialized");
        if stored_owner != owner { panic!("Unauthorized"); }

        Self::remove_admin_internal(env.clone(), admin.clone());
        env.events().publish(
            (GOV, symbol_short!("adm_rm")),
            AdminRemovedEvent { admin, removed_by: owner, timestamp: env.ledger().timestamp() },
        );
        true
    }

    pub fn is_admin(env: Env, addr: Address) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Admin(addr)).unwrap_or(false)
    }

    pub fn delegate_vote(env: Env, delegator: Address, delegatee: Address) -> bool {
        delegator.require_auth();
        if !Self::is_admin(env.clone(), delegator.clone()) { panic!("Only admins"); }
        if !Self::is_admin(env.clone(), delegatee.clone()) { panic!("Only admins"); }
        
        Self::assert_no_delegation_cycle(env.clone(), delegator.clone(), delegatee.clone());
        env.storage().persistent().set(&DataKey::Delegate(delegator.clone()), &delegatee);
        true
    }

    pub fn create_proposal(env: Env, creator: Address, prop_type: ProposalType) -> u64 {
        creator.require_auth();
        if !Self::is_admin(env.clone(), creator.clone()) { panic!("Only admins"); }

        let mut counter: u64 = env.storage().persistent().get(&DataKey::ProposalCounter).unwrap_or(0);
        counter += 1;

        let proposal = Proposal {
            id: counter,
            creator: creator.clone(),
            prop_type,
            status: ProposalStatus::Pending,
            votes_for: 0,
            votes_against: 0,
            created_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::Proposal(counter), &proposal);
        env.storage().persistent().set(&DataKey::ProposalCounter, &counter);

        env.events().publish(
            (GOV, symbol_short!("prop_new")),
            ProposalCreatedEvent { proposal_id: counter, proposer: creator, timestamp: env.ledger().timestamp() },
        );
        counter
    }

    pub fn vote(env: Env, voter: Address, proposal_id: u64, support: bool) -> bool {
        voter.require_auth();
        if !Self::is_admin(env.clone(), voter.clone()) { panic!("Only admins"); }
        if env.storage().persistent().has(&DataKey::Delegate(voter.clone())) { panic!("Delegated"); }

        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env.storage().persistent().get(&key).expect("Not found");
        if proposal.status != ProposalStatus::Pending { panic!("Not pending"); }

        let admins = Self::admin_list(env.clone());
        let mut total_weight: i128 = 0;
        
        for admin in admins.iter() {
            let voted_key = DataKey::HasVoted(proposal_id, admin.clone());
            if !env.storage().exclusive().has(&voted_key) {
                let final_delegate = Self::resolve_final_delegate(env.clone(), admin.clone(), admins.len() + 1);
                if final_delegate == voter {
                    let weight = Self::get_voter_weight(env.clone(), admin.clone());
                    total_weight += weight;
                    env.storage().exclusive().set(&voted_key, &true);
                }
            }
        }

        if total_weight == 0 { panic!("No power"); }

        if support { proposal.votes_for += total_weight; } 
        else { proposal.votes_against += total_weight; }

        env.storage().persistent().set(&key, &proposal);
        env.events().publish(
            (GOV, symbol_short!("voted")),
            VoteCastEvent { proposal_id, voter, support, weight: total_weight, timestamp: env.ledger().timestamp() },
        );
        true
    }

    pub fn execute_proposal(env: Env, caller: Address, proposal_id: u64) -> bool {
        caller.require_auth();
        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env.storage().persistent().get(&key).expect("Not found");

        if proposal.votes_for > proposal.votes_against && proposal.votes_for > 0 {
            proposal.status = ProposalStatus::Executed;
            match &proposal.prop_type {
                ProposalType::AddAdmin(new_admin) => Self::add_admin_internal(env.clone(), new_admin.clone()),
                ProposalType::RemoveAdmin(old_admin) => Self::remove_admin_internal(env.clone(), old_admin.clone()),
                ProposalType::FeeChange(new_fee) => env.storage().persistent().set(&DataKey::PlatformFee, &new_fee),
                ProposalType::ParameterUpdate(param, value) => env.storage().persistent().set(&DataKey::Parameter(param.clone()), &value),
            }
        } else {
            proposal.status = ProposalStatus::Rejected;
        }

        env.storage().persistent().set(&key, &proposal);
        env.events().publish(
            (GOV, symbol_short!("prop_exc")),
            ProposalExecutedEvent { 
                proposal_id, 
                executor: caller, 
                status: proposal.status.clone() as u32, 
                votes_for: proposal.votes_for, 
                votes_against: proposal.votes_against, 
                timestamp: env.ledger().timestamp() 
            },
        );
        true
    }

    pub fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        env.storage().persistent().get(&DataKey::Proposal(proposal_id)).expect("Not found")
    }

    // ── Internal Helpers ─────────────────────────────────────────────────────

    fn get_voter_weight(env: Env, voter: Address) -> i128 {
        let mut total_weight: i128 = 1;

        if let Some(fl_contract) = env.storage().persistent().get::<DataKey, Address>(&DataKey::FreelancerContract) {
            let get_profile_sym = soroban_sdk::Symbol::new(&env, "get_profile");
            if let Ok(Ok(profile)) = env.try_invoke_contract::<FreelancerProfile, soroban_sdk::Error>(
                &fl_contract, &get_profile_sym, soroban_sdk::vec![&env, voter.clone().into_val(&env)]
            ) {
                total_weight += (profile.total_earnings / 10_000_000) + (profile.completed_projects as i128 * 10);
            }
        }

        if let Some(token_contract) = env.storage().persistent().get::<DataKey, Address>(&DataKey::TokenContract) {
            if let Ok(Ok(balance)) = env.try_invoke_contract::<i128, soroban_sdk::Error>(
                &token_contract, &symbol_short!("balance"), soroban_sdk::vec![&env, voter.into_val(&env)]
            ) {
                total_weight += balance / 1_000_000;
            }
        }
        total_weight
    }

    fn add_admin_internal(env: Env, admin: Address) {
        if Self::is_admin(env.clone(), admin.clone()) { return; }
        env.storage().persistent().set(&DataKey::Admin(admin.clone()), &true);
        let mut admins = Self::admin_list(env.clone());
        admins.push_back(admin);
        env.storage().persistent().set(&DataKey::AdminList, &admins);
    }

    fn remove_admin_internal(env: Env, admin: Address) {
        if !Self::is_admin(env.clone(), admin.clone()) { return; }
        env.storage().persistent().remove(&DataKey::Admin(admin.clone()));
        let mut admins = Self::admin_list(env.clone());
        for i in 0..admins.len() {
            if admins.get(i).unwrap() == admin { admins.remove(i); break; }
        }
        env.storage().persistent().set(&DataKey::AdminList, &admins);
        env.storage().persistent().remove(&DataKey::Delegate(admin.clone()));
    }

    fn admin_list(env: Env) -> Vec<Address> {
        env.storage().persistent().get(&DataKey::AdminList).unwrap_or(Vec::new(&env))
    }

    fn assert_no_delegation_cycle(env: Env, delegator: Address, delegatee: Address) {
        let mut current = delegatee;
        for _ in 0..(Self::admin_list(env.clone()).len() + 1) {
            if current == delegator { panic!("Cycle"); }
            match env.storage().persistent().get(&DataKey::Delegate(current.clone())) {
                Some(next) => current = next,
                None => return,
            }
        }
        panic!("Cycle");
    }

    fn resolve_final_delegate(env: Env, start: Address, max_hops: u32) -> Address {
        let mut current = start;
        for _ in 0..max_hops {
            match env.storage().persistent().get(&DataKey::Delegate(current.clone())) {
                Some(next) => {
                    if !Self::is_admin(env.clone(), next.clone()) { return current; }
                    current = next;
                }
                None => return current,
            }
        }
        panic!("Cycle");
    }
}
```