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
    pub votes_for: i128,    // Changed to i128 for weighted voting
    pub votes_against: i128, // Changed to i128 for weighted voting
    pub created_at: u64,
}

// ── Events ───────────────────────────────────────────────────────────────────

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalCreated {
    pub proposal_id: u64,
    pub creator: Address,
}

#[soroban_sdk::contractevent]
#[derive(Clone, Debug, PartialEq)]
pub struct Voted {
    pub proposal_id: u64,
    pub voter: Address,
    pub support: bool,
    pub weight: i128,
}

const GOVERNANCE: Symbol = symbol_short!("gov");

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
        let stored_owner: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner)
            .expect("Not initialized");
        if owner != stored_owner {
            panic!("Only owner can set token contract");
        }
        env.storage()
            .persistent()
            .set(&DataKey::TokenContract, &token_contract);
        true
    }

    /// Add an admin. Only the owner may add admins.
    pub fn add_admin(env: Env, owner: Address, admin: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner)
            .expect("Governance not initialized");
        if stored_owner != owner {
            panic!("Only owner can add admins");
        }

        Self::add_admin_internal(env.clone(), admin.clone());
        env.events()
            .publish((GOVERNANCE, symbol_short!("adm_add"), admin), (owner,));
        true
    }

    /// Remove an admin. Only the owner may remove admins.
    pub fn remove_admin(env: Env, owner: Address, admin: Address) -> bool {
        owner.require_auth();
        let stored_owner: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner)
            .expect("Governance not initialized");
        if stored_owner != owner {
            panic!("Only owner can remove admins");
        }

        Self::remove_admin_internal(env.clone(), admin.clone());
        env.events()
            .publish((GOVERNANCE, symbol_short!("adm_rm"), admin), (owner,));
        true
    }

    /// Check whether an address is an admin.
    pub fn is_admin(env: Env, addr: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Admin(addr))
            .unwrap_or(false)
    }

    /// Delegation logic: Delegate the caller's voting power to another admin.
    pub fn delegate_vote(env: Env, delegator: Address, delegatee: Address) -> bool {
        delegator.require_auth();
        if !Self::is_admin(env.clone(), delegator.clone()) {
            panic!("Only admins can delegate");
        }
        if !Self::is_admin(env.clone(), delegatee.clone()) {
            panic!("Delegatee must be an admin");
        }
        if delegator == delegatee {
            panic!("Cannot delegate to self");
        }

        Self::assert_no_delegation_cycle(env.clone(), delegator.clone(), delegatee.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Delegate(delegator.clone()), &delegatee);

        env.events().publish(
            (GOVERNANCE, symbol_short!("deleg_set"), delegator),
            (delegatee,),
        );
        true
    }

    pub fn clear_delegate(env: Env, delegator: Address) -> bool {
        delegator.require_auth();
        if !Self::is_admin(env.clone(), delegator.clone()) {
            panic!("Only admins can clear delegation");
        }
        env.storage()
            .persistent()
            .remove(&DataKey::Delegate(delegator.clone()));
        true
    }

    /// Creates a new governance proposal.
    pub fn create_proposal(env: Env, creator: Address, prop_type: ProposalType) -> u64 {
        creator.require_auth();
        if !Self::is_admin(env.clone(), creator.clone()) {
            panic!("Only admins can create proposals");
        }

        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0);
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
            (GOVERNANCE, symbol_short!("prop_new"), counter),
            (creator,),
        );

        counter
    }

    /// Cast a vote on a Pending proposal. Logic upgraded for WEIGHTED voting.
    pub fn vote(env: Env, voter: Address, proposal_id: u64, support: bool) -> bool {
        voter.require_auth();
        if !Self::is_admin(env.clone(), voter.clone()) {
            panic!("Only admins can vote");
        }

        // Delegated admins cannot vote directly (their power is consolidated in their target).
        if env.storage().persistent().has(&DataKey::Delegate(voter.clone())) {
            panic!("Delegated admins cannot vote directly");
        }

        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Proposal not found");

        if proposal.status != ProposalStatus::Pending {
            panic!("Proposal is not in Pending status");
        }

        // We calculate voting power by summing the weights of the voter and all admins delegating to them.
        let admins = Self::admin_list(env.clone());
        let mut total_weight: i128 = 0;
        
        for admin in admins.iter() {
            let voted_key = DataKey::HasVoted(proposal_id, admin.clone());
            if !env.storage().persistent().has(&voted_key) {
                let final_delegate = Self::resolve_final_delegate(env.clone(), admin.clone(), admins.len() + 1);
                if final_delegate == voter {
                    let weight = Self::get_voter_weight(env.clone(), admin.clone());
                    total_weight += weight;
                    env.storage().persistent().set(&voted_key, &true);
                }
            }
        }

        if total_weight == 0 {
            panic!("No voting power available");
        }

        if support {
            proposal.votes_for += total_weight;
        } else {
            proposal.votes_against += total_weight;
        }

        env.storage().persistent().set(&key, &proposal);
        env.events().publish(
            (GOVERNANCE, symbol_short!("voted"), proposal_id),
            (voter, support, total_weight),
        );

        true
    }

    /// Executes a proposal if threshold met.
    pub fn execute_proposal(env: Env, caller: Address, proposal_id: u64) -> bool {
        caller.require_auth();
        if !Self::is_admin(env.clone(), caller.clone()) {
            panic!("Only admins can execute proposals");
        }

        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Proposal not found");

        if proposal.status != ProposalStatus::Pending {
            panic!("Proposal is not in Pending status");
        }

        if proposal.votes_for > proposal.votes_against && proposal.votes_for > 0 {
            proposal.status = ProposalStatus::Executed;
            match &proposal.prop_type {
                ProposalType::AddAdmin(new_admin) => {
                    Self::add_admin_internal(env.clone(), new_admin.clone());
                }
                ProposalType::RemoveAdmin(old_admin) => {
                    Self::remove_admin_internal(env.clone(), old_admin.clone());
                }
            }
        } else {
            proposal.status = ProposalStatus::Rejected;
        }

        env.storage().persistent().set(&key, &proposal);
        env.events().publish(
            (GOVERNANCE, symbol_short!("prop_exec"), proposal_id),
            (proposal.status as u32,),
        );

        true
    }

    pub fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .expect("Proposal not found")
    }

    // ── Internal Helpers ─────────────────────────────────────────────────────

    fn get_voter_weight(env: Env, voter: Address) -> i128 {
        let mut total_weight: i128 = 1; // Base weight

        // 1. Reputation Weight (from Freelancer Portfolio)
        if let Some(fl_contract) = env.storage().persistent().get::<DataKey, Address>(&DataKey::FreelancerContract) {
            // Fix #111: Symbol for "get_profile" (11 chars) cannot use symbol_short!
            let get_profile_sym = soroban_sdk::Symbol::new(&env, "get_profile");
            match env.try_invoke_contract::<FreelancerProfile, soroban_sdk::Error>(
                &fl_contract,
                &get_profile_sym,
                soroban_sdk::vec![&env, voter.clone().into_val(&env)],
            ) {
                Ok(Ok(profile)) => {
                    // Weighted reputation power: (total_earnings / 1e7) + (completed_projects * 10)
                    let reputation_power = (profile.total_earnings / 10_000_000) + (profile.completed_projects as i128 * 10);
                    total_weight += reputation_power;
                }
                _ => {} // No reputation bonus if profile not found or error
            }
        }

        // 2. Token Stake Weight (from Platform Token contract)
        if let Some(token_contract) = env.storage().persistent().get::<DataKey, Address>(&DataKey::TokenContract) {
            // Standard SAC (Stellar Asset Contract) / Token interface balance check
            match env.try_invoke_contract::<i128, soroban_sdk::Error>(
                &token_contract,
                &soroban_sdk::symbol_short!("balance"),
                soroban_sdk::vec![&env, voter.into_val(&env)],
            ) {
                Ok(Ok(balance)) => {
                    // Token power: 1 vote per 1,000,000 units (proportional to holding)
                    let token_power: i128 = balance / 1_000_000i128;
                    total_weight += token_power;
                }
                _ => {} // No token bonus if error or zero balance
            }
        }

        total_weight
    }

    fn add_admin_internal(env: Env, admin: Address) {
        if Self::is_admin(env.clone(), admin.clone()) {
            return;
        }
        env.storage().persistent().set(&DataKey::Admin(admin.clone()), &true);
        let mut admins = Self::admin_list(env.clone());
        admins.push_back(admin);
        env.storage().persistent().set(&DataKey::AdminList, &admins);
    }

    fn remove_admin_internal(env: Env, admin: Address) {
        if !Self::is_admin(env.clone(), admin.clone()) {
            return;
        }
        env.storage().persistent().remove(&DataKey::Admin(admin.clone()));
        let mut admins = Self::admin_list(env.clone());
        let mut i: u32 = 0;
        while i < admins.len() {
            if admins.get(i).expect("admin missing") == admin {
                admins.remove(i);
                break;
            }
            i += 1;
        }
        env.storage().persistent().set(&DataKey::AdminList, &admins);
        env.storage().persistent().remove(&DataKey::Delegate(admin.clone()));
    }

    fn admin_list(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AdminList)
            .unwrap_or(Vec::new(&env))
    }

    fn assert_no_delegation_cycle(env: Env, delegator: Address, delegatee: Address) {
        let mut current = delegatee;
        let max_hops = Self::admin_list(env.clone()).len() + 1;
        let mut hops: u32 = 0;
        while hops < max_hops {
            if current == delegator {
                panic!("Delegation cycle not allowed");
            }
            let next: Option<Address> = env.storage().persistent().get(&DataKey::Delegate(current.clone()));
            match next {
                Some(next_addr) => current = next_addr,
                None => return,
            }
            hops += 1;
        }
        panic!("Delegation cycle not allowed");
    }

    fn resolve_final_delegate(env: Env, start: Address, max_hops: u32) -> Address {
        let mut current = start;
        let mut hops: u32 = 0;
        while hops < max_hops {
            let next: Option<Address> = env.storage().persistent().get(&DataKey::Delegate(current.clone()));
            match next {
                Some(next_addr) => {
                    if !Self::is_admin(env.clone(), next_addr.clone()) {
                        return current;
                    }
                    current = next_addr;
                }
                None => return current,
            }
            hops += 1;
        }
        panic!("Delegation cycle not allowed");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    struct TestEnv {
        env: Env,
        owner: Address,
        contract_id: Address,
    }

    impl TestEnv {
        fn new() -> Self {
            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register(GovernanceContract, ());
            let owner = Address::generate(&env);
            GovernanceContractClient::new(&env, &contract_id).init(&owner);
            TestEnv { env, owner, contract_id }
        }

        fn client(&self) -> GovernanceContractClient {
            GovernanceContractClient::new(&self.env, &self.contract_id)
        }
    }

    #[test]
    fn test_init_and_admin() {
        let t = TestEnv::new();
        let client = t.client();
        let admin = Address::generate(&t.env);
        client.add_admin(&t.owner, &admin);
        assert!(client.is_admin(&admin));
    }

    #[contract]
    pub struct MockFreelancer;

    #[contractimpl]
    impl MockFreelancer {
        pub fn get_profile(env: Env, addr: Address) -> FreelancerProfile {
            FreelancerProfile {
                address: addr,
                name: soroban_sdk::String::from_str(&env, "Mock"),
                discipline: soroban_sdk::String::from_str(&env, "Mock"),
                bio: soroban_sdk::String::from_str(&env, "Mock"),
                rating: 500,
                total_rating_count: 1,
                completed_projects: 10,
                total_earnings: 20_000_000,
                verified: true,
                created_at: 0,
                skills: soroban_sdk::Vec::new(&env),
            }
        }
    }

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn balance(_env: Env, _addr: Address) -> i128 {
            5_000_000
        }
    }

    #[test]
    fn test_weighted_voting_reputation() {
        let t = TestEnv::new();
        let client = t.client();
        let admin = Address::generate(&t.env);
        client.add_admin(&t.owner, &admin);

        let freelancer_id = t.env.register(MockFreelancer, ());
        client.set_freelancer_contract(&t.owner, &freelancer_id);

        let prop_id = client.create_proposal(&admin, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin, &prop_id, &true);

        let proposal = client.get_proposal(&prop_id);
        // Base(1) + (20,000,000 / 10,000,000) + (10 * 10) = 1 + 2 + 100 = 103
        assert_eq!(proposal.votes_for, 103);
    }

    #[test]
    fn test_weighted_voting_tokens() {
        let t = TestEnv::new();
        let client = t.client();
        let admin = Address::generate(&t.env);
        client.add_admin(&t.owner, &admin);

        let token_id = t.env.register(MockToken, ());
        client.set_token_contract(&t.owner, &token_id);

        let prop_id = client.create_proposal(&admin, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin, &prop_id, &true);

        let proposal = client.get_proposal(&prop_id);
        // Base(1) + (5,000,000 / 1,000,000) = 1 + 5 = 6
        assert_eq!(proposal.votes_for, 6);
    }

    #[test]
    fn test_weighted_voting_combined() {
        let t = TestEnv::new();
        let client = t.client();
        let admin = Address::generate(&t.env);
        client.add_admin(&t.owner, &admin);

        let freelancer_id = t.env.register(MockFreelancer, ());
        let token_id = t.env.register(MockToken, ());
        client.set_freelancer_contract(&t.owner, &freelancer_id);
        client.set_token_contract(&t.owner, &token_id);

        let prop_id = client.create_proposal(&admin, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin, &prop_id, &true);

        let proposal = client.get_proposal(&prop_id);
        // Base(1) + Rep(2 + 100) + Tok(5) = 1 + 102 + 5 = 108
        assert_eq!(proposal.votes_for, 108);
    }

    #[test]
    fn test_weighted_voting_base() {
        let t = TestEnv::new();
        let client = t.client();
        let admin1 = Address::generate(&t.env);
        let admin2 = Address::generate(&t.env);
        
        client.add_admin(&t.owner, &admin1);
        client.add_admin(&t.owner, &admin2);

        let prop_id = client.create_proposal(&admin1, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin1, &prop_id, &true);
        
        let proposal = client.get_proposal(&prop_id);
        // Default weight is 1.
        assert_eq!(proposal.votes_for, 1);
    }

    #[test]
    fn test_weighted_voting_with_delegation() {
        let t = TestEnv::new();
        let client = t.client();
        let admin1 = Address::generate(&t.env);
        let admin2 = Address::generate(&t.env);
        let admin3 = Address::generate(&t.env);
        
        client.add_admin(&t.owner, &admin1);
        client.add_admin(&t.owner, &admin2);
        client.add_admin(&t.owner, &admin3);

        client.delegate_vote(&admin2, &admin1);
        client.delegate_vote(&admin3, &admin1);

        let prop_id = client.create_proposal(&admin1, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin1, &prop_id, &true);
        
        let proposal = client.get_proposal(&prop_id);
        // Default base weights: 1 + 1 + 1 = 3.
        assert_eq!(proposal.votes_for, 3);
    }

    #[test]
    #[should_panic(expected = "Delegated admins cannot vote directly")]
    fn test_delegator_cannot_vote() {
        let t = TestEnv::new();
        let client = t.client();
        let admin1 = Address::generate(&t.env);
        let admin2 = Address::generate(&t.env);
        client.add_admin(&t.owner, &admin1);
        client.add_admin(&t.owner, &admin2);
        client.delegate_vote(&admin2, &admin1);

        let prop_id = client.create_proposal(&admin1, &ProposalType::AddAdmin(Address::generate(&t.env)));
        client.vote(&admin2, &prop_id, &true);
    }
}
