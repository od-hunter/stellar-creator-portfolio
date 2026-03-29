#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, IntoVal, String, Symbol, Vec};

#[contracttype]
#[derive(Clone, Debug)]
pub struct FreelancerProfile {
    pub address: Address,
    pub name: String,
    pub discipline: String,
    pub bio: String,
    pub rating: u32,
    pub total_rating_count: u32,
    pub completed_projects: u32,
    pub total_earnings: i128,
    pub verified: bool,
    pub created_at: u64,
    pub skills: Vec<String>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FilterOptions {
    pub discipline: Option<String>,
    pub min_rating: Option<u32>,
    pub verified_only: Option<bool>,
    pub skill: Option<String>,
}

#[contracttype]
pub enum DataKey {
    FreelancerCount,
    Profile(Address),
    AllFreelancers,
    Governance,
    Deployer,
    // Trusted escrow contract allowed to call update_earnings
    EscrowContract,
}

// =============================================================================
// SECURITY INVARIANTS (for formal verification / audit reference)
// =============================================================================
// INV-1: A freelancer address maps to at most one profile (duplicate registration
//        returns false without overwriting).
// INV-2: FreelancerCount equals the number of unique registered addresses.
// INV-3: Rating is a running average; total_rating_count is monotonically increasing.
// INV-4: verify_freelancer requires admin authentication; if a governance contract
//        is configured, the caller must also pass the is_admin check there.
// INV-5: set_governance_contract is restricted to the first setter (deployer),
//        preventing governance hijacking after initial configuration.
// =============================================================================

#[contract]
pub struct FreelancerContract;

// Shared topic prefix for all freelancer events — allows indexers to filter by contract.
const FL: Symbol = symbol_short!("fl"); 

#[contractimpl]
impl FreelancerContract {
    /// Registers a new freelancer profile.
    pub fn register_freelancer(
        env: Env,
        freelancer: Address,
        name: String,
        discipline: String,
        bio: String,
    ) -> bool {
        freelancer.require_auth();

        let key = DataKey::Profile(freelancer.clone());
        if env.storage().persistent().has(&key) {
            return false;
        }

        let timestamp = env.ledger().timestamp();
        let profile = FreelancerProfile {
            address: freelancer.clone(),
            name: name.clone(),
            discipline,
            bio,
            rating: 0,
            total_rating_count: 0,
            completed_projects: 0,
            total_earnings: 0,
            verified: false,
            created_at: timestamp,
            skills: Vec::new(&env),
        };

        env.storage().persistent().set(&key, &profile);

        let mut freelancers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllFreelancers)
            .unwrap_or(Vec::new(&env));
        freelancers.push_back(freelancer.clone());
        env.storage().persistent().set(&DataKey::AllFreelancers, &freelancers);

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FreelancerCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::FreelancerCount, &(count + 1));

        env.events().publish(
            (FL, symbol_short!("reg"), freelancer),
            (name, timestamp),
        );

        true
    }

    /// Retrieves freelancer profile.
    pub fn get_profile(env: Env, freelancer: Address) -> FreelancerProfile {
        env.storage()
            .persistent()
            .get(&DataKey::Profile(freelancer))
            .expect("Freelancer not registered")
    }

    pub fn update_rating(env: Env, freelancer: Address, new_rating: u32) -> bool {
        let key = DataKey::Profile(freelancer.clone());
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Freelancer not registered");

        let total = (profile.rating as u64) * (profile.total_rating_count as u64);
        profile.total_rating_count += 1;
        profile.rating = ((total + new_rating as u64) / profile.total_rating_count as u64) as u32;

        env.storage().persistent().set(&key, &profile);

        env.events().publish(
            (FL, symbol_short!("rate"), freelancer),
            (profile.rating, profile.total_rating_count),
        );

        true
    }

    /// Increments freelancer's completed projects count.
    pub fn update_completed_projects(env: Env, freelancer: Address) -> bool {
        let key = DataKey::Profile(freelancer);
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Freelancer not registered");

        profile.completed_projects += 1;
        env.storage().persistent().set(&key, &profile);
        true
    }

    /// Adds to freelancer's total earnings.
    pub fn update_earnings(env: Env, escrow: Address, freelancer: Address, amount: i128) -> bool {
        escrow.require_auth();

        let registered_escrow: Address = env
            .storage()
            .persistent()
            .get(&DataKey::EscrowContract)
            .expect("Escrow contract not configured");

        if escrow != registered_escrow {
            panic!("Unauthorized: only escrow contract may update earnings");
        }

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let key = DataKey::Profile(freelancer.clone());
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Freelancer not registered");

        profile.total_earnings += amount;
        env.storage().persistent().set(&key, &profile);

        let new_total = profile.total_earnings;

        env.events().publish(
            (FL, symbol_short!("earnings"), freelancer),
            (amount, new_total),
        );

        true
    }

    /// Registers the trusted escrow contract address.
    pub fn set_escrow_contract(env: Env, setter: Address, escrow: Address) -> bool {
        setter.require_auth();

        let maybe_deployer: Option<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Deployer);

        if let Some(deployer) = maybe_deployer {
            if deployer != setter {
                panic!("Only deployer may set escrow contract");
            }
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Deployer, &setter);
        }

        env.storage()
            .persistent()
            .set(&DataKey::EscrowContract, &escrow);

        true
    }

    /// Admin verifies freelancer (sets verified flag).
    pub fn verify_freelancer(env: Env, admin: Address, freelancer: Address) -> bool {
        admin.require_auth();

        if let Some(gov) = env.storage().persistent().get::<DataKey, Address>(&DataKey::Governance) {
            let args = soroban_sdk::vec![&env, admin.clone().into_val(&env)];
            let is_admin: bool = env.invoke_contract(&gov, &symbol_short!("is_admin"), args);
            if !is_admin {
                panic!("Admin role required");
            }
        }

        let key = DataKey::Profile(freelancer.clone());
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Freelancer not registered");

        profile.verified = true;
        env.storage().persistent().set(&key, &profile);

        env.events().publish(
            (FL, symbol_short!("ver"), freelancer),
            (admin, true),
        );

        true
    }

    /// Sets the governance contract address used for admin role checks.
    pub fn set_governance_contract(env: Env, setter: Address, governance: Address) -> bool {
        setter.require_auth();

        let maybe_deployer: Option<Address> = env.storage().persistent().get(&DataKey::Deployer);
        if let Some(deployer) = maybe_deployer {
            if deployer != setter {
                panic!("Only deployer may set governance contract");
            }
        } else {
            env.storage().persistent().set(&DataKey::Deployer, &setter);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Governance, &governance);
        true
    }

    /// Checks if freelancer is verified.
    pub fn is_verified(env: Env, freelancer: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, FreelancerProfile>(&DataKey::Profile(freelancer))
            .map(|p| p.verified)
            .unwrap_or(false)
    }

    pub fn get_freelancers_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FreelancerCount)
            .unwrap_or(0)
    }

    pub fn add_skill(env: Env, freelancer: Address, skill: String) -> bool {
        freelancer.require_auth();
        let key = DataKey::Profile(freelancer.clone());
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("not found");

        for s in profile.skills.iter() {
            if s == skill {
                return false;
            }
        }

        profile.skills.push_back(skill.clone());
        env.storage().persistent().set(&key, &profile);

        env.events().publish(
            (FL, symbol_short!("sk_add"), freelancer),
            skill,
        );
        true
    }

    pub fn remove_skill(env: Env, freelancer: Address, skill: String) -> bool {
        freelancer.require_auth();
        let key = DataKey::Profile(freelancer.clone());
        let mut profile: FreelancerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .expect("not found");

        let mut index = None;
        for (i, s) in profile.skills.iter().enumerate() {
            if s == skill {
                index = Some(i as u32);
                break;
            }
        }

        if let Some(i) = index {
            profile.skills.remove(i);
            env.storage().persistent().set(&key, &profile);
            env.events().publish(
                (FL, symbol_short!("sk_rem"), freelancer),
                skill,
            );
            true
        } else {
            false
        }
    }

    pub fn query_freelancers(env: Env, filters: FilterOptions) -> Vec<FreelancerProfile> {
        let freelancers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllFreelancers)
            .unwrap_or(Vec::new(&env));
        let mut result = Vec::new(&env);

        for freelancer in freelancers.iter() {
            if let Some(profile) = env
                .storage()
                .persistent()
                .get::<DataKey, FreelancerProfile>(&DataKey::Profile(freelancer))
            {
                if let Some(ref discipline) = filters.discipline {
                    if profile.discipline != *discipline { continue; }
                }
                if let Some(min_rating) = filters.min_rating {
                    if profile.rating < min_rating { continue; }
                }
                if let Some(verified_only) = filters.verified_only {
                    if verified_only && !profile.verified { continue; }
                }
                if let Some(ref skill) = filters.skill {
                    let mut has_skill = false;
                    for s in profile.skills.iter() {
                        if s == *skill { has_skill = true; break; }
                    }
                    if !has_skill { continue; }
                }
                result.push_back(profile);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    #[test]
    fn test_full_workflow() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);
        let freelancer = Address::generate(&env);

        // Register
        client.register_freelancer(&freelancer, &String::from_str(&env, "Alice"), &String::from_str(&env, "Design"), &String::from_str(&env, "Bio"));
        
        // Add skill
        let skill = String::from_str(&env, "Rust");
        client.add_skill(&freelancer, &skill);
        assert_eq!(client.get_profile(&freelancer).skills.len(), 1);

        // Update rating
        client.update_rating(&freelancer, &5);
        assert_eq!(client.get_profile(&freelancer).rating, 5);

        // Verify
        let admin = Address::generate(&env);
        client.verify_freelancer(&admin, &freelancer);
        assert!(client.is_verified(&freelancer));

        // Query
        let filters = FilterOptions {
            discipline: None,
            min_rating: Some(4),
            verified_only: Some(true),
            skill: Some(skill),
        };
        let result = client.query_freelancers(&filters);
        assert_eq!(result.len(), 1);
        
        // Remove skill
        client.remove_skill(&freelancer, &String::from_str(&env, "Rust"));
        assert_eq!(client.get_profile(&freelancer).skills.len(), 0);
    }

    #[test]
    fn test_update_earnings_success() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let deployer = Address::generate(&env);
        let escrow = Address::generate(&env);
        let freelancer = Address::generate(&env);

        client.set_escrow_contract(&deployer, &escrow);

        client.register_freelancer(
            &freelancer,
            &String::from_str(&env, "Alice"),
            &String::from_str(&env, "Design"),
            &String::from_str(&env, "Bio"),
        );

        let result = client.update_earnings(&escrow, &freelancer, &500i128);
        assert!(result);

        let profile = client.get_profile(&freelancer);
        assert_eq!(profile.total_earnings, 500i128);

        client.update_earnings(&escrow, &freelancer, &250i128);
        let profile = client.get_profile(&freelancer);
        assert_eq!(profile.total_earnings, 750i128);
    }

    #[test]
    #[should_panic(expected = "Unauthorized: only escrow contract may update earnings")]
    fn test_update_earnings_unauthorized_caller() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let deployer = Address::generate(&env);
        let escrow = Address::generate(&env);
        let attacker = Address::generate(&env);
        let freelancer = Address::generate(&env);

        client.set_escrow_contract(&deployer, &escrow);
        client.register_freelancer(
            &freelancer,
            &String::from_str(&env, "Alice"),
            &String::from_str(&env, "Design"),
            &String::from_str(&env, "Bio"),
        );

        client.update_earnings(&attacker, &freelancer, &9999i128);
    }

    #[test]
    #[should_panic(expected = "Escrow contract not configured")]
    fn test_update_earnings_no_escrow_configured() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let escrow = Address::generate(&env);
        let freelancer = Address::generate(&env);

        client.register_freelancer(
            &freelancer,
            &String::from_str(&env, "Alice"),
            &String::from_str(&env, "Design"),
            &String::from_str(&env, "Bio"),
        );

        client.update_earnings(&escrow, &freelancer, &100i128);
    }

    #[test]
    #[should_panic(expected = "Amount must be positive")]
    fn test_update_earnings_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let deployer = Address::generate(&env);
        let escrow = Address::generate(&env);
        let freelancer = Address::generate(&env);

        client.set_escrow_contract(&deployer, &escrow);
        client.register_freelancer(
            &freelancer,
            &String::from_str(&env, "Alice"),
            &String::from_str(&env, "Design"),
            &String::from_str(&env, "Bio"),
        );

        client.update_earnings(&escrow, &freelancer, &0i128);
    }

    #[test]
    #[should_panic(expected = "Amount must be positive")]
    fn test_update_earnings_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let deployer = Address::generate(&env);
        let escrow = Address::generate(&env);
        let freelancer = Address::generate(&env);

        client.set_escrow_contract(&deployer, &escrow);
        client.register_freelancer(
            &freelancer,
            &String::from_str(&env, "Alice"),
            &String::from_str(&env, "Design"),
            &String::from_str(&env, "Bio"),
        );

        client.update_earnings(&escrow, &freelancer, &-100i128);
    }

    #[test]
    #[should_panic(expected = "Only deployer may set escrow contract")]
    fn test_set_escrow_contract_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(FreelancerContract, ());
        let client = FreelancerContractClient::new(&env, &contract_id);

        let deployer = Address::generate(&env);
        let escrow = Address::generate(&env);
        let attacker = Address::generate(&env);
        let new_escrow = Address::generate(&env);

        client.set_escrow_contract(&deployer, &escrow);

        client.set_escrow_contract(&attacker, &new_escrow);
    }
}
