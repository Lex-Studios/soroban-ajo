use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

use crate::errors::AjoError;
use crate::events;
use crate::pausable;
use crate::storage;
use crate::types::{Group, GroupMetadata, GroupStatus};
use crate::utils;

/// The main Ajo contract
#[contract]
pub struct AjoContract;

#[contractimpl]
impl AjoContract {
    /// Initialize the contract with an admin.
    ///
    /// This function must be called exactly once to set up the contract's admin.
    /// After initialization, the admin can upgrade the contract.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `admin` - Address of the contract administrator
    ///
    /// # Returns
    /// `Ok(())` on successful initialization
    ///
    /// # Errors
    /// * `AlreadyInitialized` - If the contract has already been initialized
    pub fn initialize(env: Env, admin: Address) -> Result<(), AjoError> {
        if storage::get_admin(&env).is_some() {
            return Err(AjoError::AlreadyInitialized);
        }
        storage::store_admin(&env, &admin);
        Ok(())
    }

    /// Upgrade the contract's Wasm bytecode.
    ///
    /// Only the admin can call this function. The contract will be updated to the
    /// new Wasm code specified by `new_wasm_hash`.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `new_wasm_hash` - The hash of the new Wasm code (32 bytes)
    ///
    /// # Returns
    /// `Ok(())` on successful upgrade
    ///
    /// # Errors
    /// * `Unauthorized` - If the caller is not the admin
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), AjoError> {
        let admin = storage::get_admin(&env).ok_or(AjoError::Unauthorized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Pause the contract to prevent state-mutating operations.
    ///
    /// This emergency function allows the admin to temporarily halt all state-mutating
    /// operations (create_group, join_group, contribute, execute_payout) while keeping
    /// query functions and admin functions operational. This is useful during security
    /// incidents, detected vulnerabilities, or maintenance periods.
    ///
    /// When paused:
    /// - All state-mutating operations will fail with `ContractPaused` error
    /// - Query operations continue to work normally
    /// - Admin operations (pause, unpause, upgrade) remain available
    /// - All stored data (groups, contributions, payouts) remains safe and intact
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    ///
    /// # Returns
    /// `Ok(())` on successful pause
    ///
    /// # Errors
    /// * `UnauthorizedPause` - If the caller is not the admin
    ///
    /// # Authorization
    /// Only the contract admin can call this function.
    pub fn pause(env: Env) -> Result<(), AjoError> {
        pausable::pause(&env)
    }

    /// Unpause the contract to restore normal operations.
    ///
    /// This function allows the admin to restore full contract functionality after
    /// an emergency pause. Once unpaused, all state-mutating operations return to
    /// normal operation. All data remains intact and accessible.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    ///
    /// # Returns
    /// `Ok(())` on successful unpause
    ///
    /// # Errors
    /// * `UnauthorizedUnpause` - If the caller is not the admin
    ///
    /// # Authorization
    /// Only the contract admin can call this function.
    ///
    /// # Data Safety
    /// Unpausing does not modify any stored data. All groups, contributions, and
    /// payouts remain exactly as they were before the pause.
    pub fn unpause(env: Env) -> Result<(), AjoError> {
        pausable::unpause(&env)
    }

    /// Create a new Ajo group.
    ///
    /// Initializes a new rotating savings group with the specified parameters.
    /// The creator becomes the first member and the contract validates all parameters
    /// before storage. A unique group ID is assigned and returned.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `creator` - Address of the group creator (automatically becomes first member)
    /// * `contribution_amount` - Fixed amount each member contributes per cycle (in stroops, must be > 0)
    /// * `cycle_duration` - Duration of each cycle in seconds (must be > 0)
    /// * `max_members` - Maximum number of members allowed in the group (must be >= 2 and <= 100)
    /// * `grace_period` - Grace period duration in seconds after cycle ends (default: 86400 = 24 hours)
    /// * `penalty_rate` - Penalty rate as percentage for late contributions (0-100, default: 5)
    ///
    /// # Returns
    /// The unique group ID assigned to the new group
    ///
    /// # Errors
    /// * `ContributionAmountZero` - If contribution_amount == 0
    /// * `ContributionAmountNegative` - If contribution_amount < 0
    /// * `CycleDurationZero` - If cycle_duration == 0
    /// * `MaxMembersBelowMinimum` - If max_members < 2
    /// * `MaxMembersAboveLimit` - If max_members > 100
    /// * `InvalidGracePeriod` - If grace_period > 7 days
    /// * `InvalidPenaltyRate` - If penalty_rate > 100
    pub fn create_group(
        env: Env,
        creator: Address,
        contribution_amount: i128,
        cycle_duration: u64,
        max_members: u32,
        grace_period: u64,
        penalty_rate: u32,
    ) -> Result<u64, AjoError> {
        // Validate parameters
        utils::validate_group_params(contribution_amount, cycle_duration, max_members)?;
        utils::validate_penalty_params(grace_period, penalty_rate)?;

        // Check if paused
        pausable::ensure_not_paused(&env)?;

        // Require authentication
        creator.require_auth();

        // Generate new group ID
        let group_id = storage::get_next_group_id(&env);

        // Initialize members list with creator
        let mut members = Vec::new(&env);
        members.push_back(creator.clone());

        // Get current timestamp
        let now = utils::get_current_timestamp(&env);

        // Create group
        let group = Group {
            id: group_id,
            creator: creator.clone(),
            contribution_amount,
            cycle_duration,
            max_members,
            members,
            current_cycle: 1,
            payout_index: 0,
            created_at: now,
            cycle_start_time: now,
            is_complete: false,
            grace_period,
            penalty_rate,
        };

        // Store group
        storage::store_group(&env, group_id, &group);

        // Emit event
        events::emit_group_created(&env, group_id, &creator, contribution_amount, max_members);

        Ok(group_id)
    }

    /// Get group information.
    ///
    /// Retrieves the complete group data including all members, cycle information,
    /// and metadata.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    ///
    /// # Returns
    /// The group data containing group configuration and current state
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn get_group(env: Env, group_id: u64) -> Result<Group, AjoError> {
        storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)
    }

    /// Get list of all members in a group.
    ///
    /// Returns the ordered list of all member addresses currently in the group.
    /// Members are ordered by join time, with the creator being the first member.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    ///
    /// # Returns
    /// Vector of member addresses in join order
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn list_members(env: Env, group_id: u64) -> Result<Vec<Address>, AjoError> {
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;
        Ok(group.members)
    }

    /// Join an existing group.
    ///
    /// Adds a new member to an active group if space is available.
    /// The member's authentication is required. The member cannot join if they
    /// are already a member, the group is full, or the group has completed all cycles.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `member` - Address of the member joining (must authenticate)
    /// * `group_id` - The group to join
    ///
    /// # Returns
    /// `Ok(())` on successful group join
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    /// * `MaxMembersExceeded` - If the group has reached max members
    /// * `AlreadyMember` - If the address is already a member
    /// * `GroupComplete` - If the group has completed all cycles
    pub fn join_group(env: Env, member: Address, group_id: u64) -> Result<(), AjoError> {
        // Check if paused
        pausable::ensure_not_paused(&env)?;

        // Require authentication
        member.require_auth();

        // Get group
        let mut group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Check if group is complete
        if group.is_complete {
            return Err(AjoError::GroupComplete);
        }

        // Check if already a member
        if utils::is_member(&group.members, &member) {
            return Err(AjoError::AlreadyMember);
        }

        // Check if group is full
        if group.members.len() >= group.max_members {
            return Err(AjoError::MaxMembersExceeded);
        }

        // Add member
        group.members.push_back(member.clone());

        // Update storage
        storage::store_group(&env, group_id, &group);

        // Emit event
        events::emit_member_joined(&env, group_id, &member);

        Ok(())
    }

    /// Check if an address is a member of a group.
    ///
    /// Returns whether the provided address is currently a member of the specified group.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The group to check
    /// * `address` - The address to check
    ///
    /// # Returns
    /// `true` if the address is a member, `false` otherwise
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn is_member(env: Env, group_id: u64, address: Address) -> Result<bool, AjoError> {
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;
        Ok(utils::is_member(&group.members, &address))
    }

    /// Contribute to the current cycle.
    ///
    /// Records a member's contribution for the current cycle. Each member can contribute
    /// once per cycle. Authentication is required. Contributions are recorded but actual
    /// fund transfers are handled by external payment systems.
    ///
    /// Late contributions (after cycle ends but within grace period) incur penalties.
    /// Contributions after grace period are rejected.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `member` - Address making the contribution (must authenticate)
    /// * `group_id` - The group to contribute to
    ///
    /// # Returns
    /// `Ok(())` on successful contribution recording
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    /// * `NotMember` - If the address is not a member
    /// * `AlreadyContributed` - If already contributed this cycle
    /// * `GroupComplete` - If the group has completed all cycles
    /// * `GracePeriodExpired` - If contribution is too late (after grace period)
    pub fn contribute(env: Env, member: Address, group_id: u64) -> Result<(), AjoError> {
        // Check if paused
        pausable::ensure_not_paused(&env)?;

        // Require authentication
        member.require_auth();

        // Get group
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Check if group is complete
        if group.is_complete {
            return Err(AjoError::GroupComplete);
        }

        // Check if member
        if !utils::is_member(&group.members, &member) {
            return Err(AjoError::NotMember);
        }

        // Check if already contributed
        if storage::has_contributed(&env, group_id, group.current_cycle, &member) {
            return Err(AjoError::AlreadyContributed);
        }

        // Get current time and check contribution timing
        let current_time = utils::get_current_timestamp(&env);
        let (is_late, penalty_amount) = utils::check_contribution_timing(&group, current_time);

        // If too late (after grace period), reject
        if is_late && penalty_amount == 0 {
            // This means we're past grace period
            if !utils::is_within_grace_period(&group, current_time) {
                return Err(AjoError::GracePeriodExpired);
            }
        }

        // Transfer contribution to contract
        // Note: In production, this would use token.transfer() or native transfer
        // For now, we mark as contributed (assuming payment succeeded)

        // Record contribution with details
        let contribution_record = crate::types::ContributionRecord {
            member: member.clone(),
            group_id,
            cycle: group.current_cycle,
            has_paid: true,
            timestamp: current_time,
            is_late,
            penalty_amount,
        };
        storage::store_contribution_detail(&env, group_id, group.current_cycle, &member, &contribution_record);

        // Also store simple boolean flag for backward compatibility
        storage::store_contribution(&env, group_id, group.current_cycle, &member, true);

        // If late, add penalty to pool and update member record
        if is_late && penalty_amount > 0 {
            storage::add_to_penalty_pool(&env, group_id, group.current_cycle, penalty_amount);
            utils::update_member_penalty_record(&env, group_id, &member, true, penalty_amount);

            // Emit late contribution event
            events::emit_late_contribution(
                &env,
                group_id,
                &member,
                group.current_cycle,
                group.contribution_amount,
                penalty_amount,
            );
        } else {
            // Update member record for on-time contribution
            utils::update_member_penalty_record(&env, group_id, &member, false, 0);

            // Emit regular contribution event
            events::emit_contribution_made(
                &env,
                group_id,
                &member,
                group.current_cycle,
                group.contribution_amount,
            );
        }

        Ok(())
    }

    /// Get contribution status for all members in a specific cycle.
    ///
    /// Returns an ordered list of all members paired with their contribution status
    /// for the specified cycle. Member order matches the group's member list order.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The group to check
    /// * `cycle_number` - The cycle to check (typically use current_cycle from group)
    ///
    /// # Returns
    /// Vector of (Address, bool) tuples where `true` indicates the member has contributed
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn get_contribution_status(
        env: Env,
        group_id: u64,
        cycle_number: u32,
    ) -> Result<Vec<(Address, bool)>, AjoError> {
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;
        Ok(storage::get_cycle_contributions(
            &env,
            group_id,
            cycle_number,
            &group.members,
        ))
    }

    /// Execute payout for the current cycle.
    ///
    /// This is the core function that rotates payouts through group members.
    /// It verifies that all members have contributed, calculates the total payout
    /// (including any penalties collected), distributes funds to the next recipient,
    /// and advances the cycle. When all members have received their payout, the group
    /// is marked complete.
    ///
    /// Payout can only be executed after the grace period expires to ensure all
    /// late contributions are collected.
    ///
    /// Process:
    /// 1. Verifies all members have contributed in the current cycle
    /// 2. Ensures grace period has expired
    /// 3. Calculates total payout (contribution_amount × member_count + penalties)
    /// 4. Records payout to the current recipient
    /// 5. Emits payout event with penalty bonus
    /// 6. Advances to next cycle (or marks complete if done)
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The group to execute payout for
    ///
    /// # Returns
    /// `Ok(())` on successful payout execution
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    /// * `IncompleteContributions` - If not all members have contributed
    /// * `GroupComplete` - If the group has already completed all payouts
    /// * `NoMembers` - If the group has no members (should never happen)
    /// * `OutsideCycleWindow` - If grace period has not expired yet
    pub fn execute_payout(env: Env, group_id: u64) -> Result<(), AjoError> {
        // Check if paused
        pausable::ensure_not_paused(&env)?;

        // Get group
        let mut group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Check if group is complete
        if group.is_complete {
            return Err(AjoError::GroupComplete);
        }

        // Check if all members have contributed
        if !utils::all_members_contributed(&env, &group) {
            return Err(AjoError::IncompleteContributions);
        }

        // Ensure grace period has expired before executing payout
        let current_time = utils::get_current_timestamp(&env);
        let grace_end = utils::get_grace_period_end(&group);
        if current_time < grace_end {
            // Still within grace period - delay payout
            return Err(AjoError::OutsideCycleWindow);
        }

        // Get payout recipient
        let payout_recipient = group
            .members
            .get(group.payout_index)
            .ok_or(AjoError::NoMembers)?;

        // Calculate base payout amount
        let base_payout = utils::calculate_payout_amount(&group);

        // Get penalty pool for this cycle
        let penalty_bonus = storage::get_cycle_penalty_pool(&env, group_id, group.current_cycle);

        // Total payout includes penalties
        let total_payout = base_payout + penalty_bonus;

        // Transfer payout to recipient
        // Note: In production, this would use token.transfer() or native transfer
        // For now, we just record it

        // Mark payout as received
        storage::mark_payout_received(&env, group_id, &payout_recipient);

        // Emit payout event with penalty information
        if penalty_bonus > 0 {
            events::emit_penalty_distributed(
                &env,
                group_id,
                &payout_recipient,
                group.current_cycle,
                base_payout,
                penalty_bonus,
            );
        }

        events::emit_payout_executed(
            &env,
            group_id,
            &payout_recipient,
            group.current_cycle,
            total_payout,
        );

        // Advance payout index
        group.payout_index += 1;

        // Check if all members have received payout
        if group.payout_index >= group.members.len() {
            // All members have received payout - mark complete
            group.is_complete = true;
            events::emit_group_completed(&env, group_id);
        } else {
            // Advance to next cycle
            group.current_cycle += 1;
            group.cycle_start_time = utils::get_current_timestamp(&env);
        }

        // Update storage
        storage::store_group(&env, group_id, &group);

        Ok(())
    }

    /// Check if a group has completed all cycles.
    ///
    /// Returns whether the group has completed its full rotation,
    /// meaning all members have received at least one payout.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The group to check
    ///
    /// # Returns
    /// `true` if the group has completed all payouts, `false` otherwise
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn is_complete(env: Env, group_id: u64) -> Result<bool, AjoError> {
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;
        Ok(group.is_complete)
    }

    /// Get comprehensive group status.
    ///
    /// Returns a detailed snapshot of the group's current state, including cycle
    /// information, contribution status, payout progress, and timing data.
    /// This is the primary function for checking a group's overall progress.
    ///
    /// Returns information about:
    /// - Current cycle number and progress
    /// - Next recipient for payout
    /// - Members who have contributed and those who are pending
    /// - Cycle timing (start time, end time, whether cycle is active)
    /// - Whether the group is complete
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    ///
    /// # Returns
    /// A `GroupStatus` struct containing comprehensive group state information
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    pub fn get_group_status(env: Env, group_id: u64) -> Result<GroupStatus, AjoError> {
        // Get the group data
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Get current timestamp
        let current_time = utils::get_current_timestamp(&env);

        // Calculate cycle timing
        let cycle_end_time = group.cycle_start_time + group.cycle_duration;
        let grace_period_end_time = utils::get_grace_period_end(&group);
        let is_cycle_active = current_time < cycle_end_time;
        let is_in_grace_period = utils::is_within_grace_period(&group, current_time);

        // Get penalty pool for current cycle
        let cycle_penalty_pool = storage::get_cycle_penalty_pool(&env, group_id, group.current_cycle);

        // Get contribution status for all members in current cycle
        let contributions =
            storage::get_cycle_contributions(&env, group_id, group.current_cycle, &group.members);

        // Count contributions and build pending list
        let mut contributions_received: u32 = 0;
        let mut pending_contributors = Vec::new(&env);

        for (member, has_contributed) in contributions.iter() {
            if has_contributed {
                contributions_received += 1;
            } else {
                pending_contributors.push_back(member);
            }
        }

        // Determine next recipient
        let (has_next_recipient, next_recipient) = if group.is_complete {
            // Use placeholder (creator) when complete
            (false, group.creator.clone())
        } else {
            // Get the member at payout_index
            let recipient = group
                .members
                .get(group.payout_index)
                .unwrap_or_else(|| group.creator.clone());
            (true, recipient)
        };

        // Build and return status
        Ok(GroupStatus {
            group_id: group.id,
            current_cycle: group.current_cycle,
            has_next_recipient,
            next_recipient,
            contributions_received,
            total_members: group.members.len() as u32,
            pending_contributors,
            is_complete: group.is_complete,
            is_cycle_active,
            cycle_start_time: group.cycle_start_time,
            cycle_end_time,
            current_time,
            cycle_penalty_pool,
            is_in_grace_period,
            grace_period_end_time,
        })
    }

    /// Set or update metadata for an Ajo group.
    ///
    /// Only the group creator can set or update metadata.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    /// * `name` - The name of the group
    /// * `description` - The description of the group
    /// * `rules` - The rules of the group
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group does not exist
    /// * `Unauthorized` - If the caller is not the group creator
    /// * `MetadataTooLong` - If any field exceeds its length limit
    pub fn set_group_metadata(
        env: Env,
        group_id: u64,
        name: soroban_sdk::String,
        description: soroban_sdk::String,
        rules: soroban_sdk::String,
    ) -> Result<(), AjoError> {
        // Validate lengths
        if name.len() > crate::types::MAX_NAME_LENGTH
            || description.len() > crate::types::MAX_DESCRIPTION_LENGTH
            || rules.len() > crate::types::MAX_RULES_LENGTH
        {
            return Err(AjoError::MetadataTooLong);
        }

        // Get group to verify existence and check creator
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Require creator authentication
        group.creator.require_auth();

        // Create and store metadata
        let metadata = GroupMetadata {
            name,
            description,
            rules,
        };
        storage::store_group_metadata(&env, group_id, &metadata);

        Ok(())
    }

    /// Get metadata for an Ajo group.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    ///
    /// # Returns
    /// The group metadata
    ///
    /// # Errors
    /// * `GroupNotFound` - If metadata for the group doesn't exist
    pub fn get_group_metadata(env: Env, group_id: u64) -> Result<GroupMetadata, AjoError> {
        storage::get_group_metadata(&env, group_id).ok_or(AjoError::GroupNotFound)
    }

    /// Get member penalty statistics.
    ///
    /// Returns the penalty record for a member in a specific group, including
    /// late contribution count, on-time count, total penalties paid, and reliability score.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    /// * `member` - The member's address
    ///
    /// # Returns
    /// The member's penalty record
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group doesn't exist
    /// * `NotMember` - If the address is not a member of the group
    pub fn get_member_penalty_record(
        env: Env,
        group_id: u64,
        member: Address,
    ) -> Result<crate::types::MemberPenaltyRecord, AjoError> {
        // Verify group exists
        let group = storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        // Verify member
        if !utils::is_member(&group.members, &member) {
            return Err(AjoError::NotMember);
        }

        // Get penalty record or return default
        Ok(storage::get_member_penalty(&env, group_id, &member).unwrap_or(
            crate::types::MemberPenaltyRecord {
                member: member.clone(),
                group_id,
                late_count: 0,
                on_time_count: 0,
                total_penalties: 0,
                reliability_score: 100,
            },
        ))
    }

    /// Get detailed contribution record for a member in a specific cycle.
    ///
    /// Returns the full contribution record including timing, penalty information,
    /// and whether the contribution was late.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    /// * `cycle` - The cycle number
    /// * `member` - The member's address
    ///
    /// # Returns
    /// The detailed contribution record
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group doesn't exist or contribution record doesn't exist
    pub fn get_contribution_detail(
        env: Env,
        group_id: u64,
        cycle: u32,
        member: Address,
    ) -> Result<crate::types::ContributionRecord, AjoError> {
        storage::get_contribution_detail(&env, group_id, cycle, &member)
            .ok_or(AjoError::GroupNotFound)
    }

    /// Get the penalty pool for a specific cycle.
    ///
    /// Returns the total penalties collected during a cycle, which will be
    /// distributed to the payout recipient along with regular contributions.
    ///
    /// # Arguments
    /// * `env` - The Soroban contract environment
    /// * `group_id` - The unique group identifier
    /// * `cycle` - The cycle number
    ///
    /// # Returns
    /// Total penalty amount in stroops
    ///
    /// # Errors
    /// * `GroupNotFound` - If the group doesn't exist
    pub fn get_cycle_penalty_pool(env: Env, group_id: u64, cycle: u32) -> Result<i128, AjoError> {
        // Verify group exists
        storage::get_group(&env, group_id).ok_or(AjoError::GroupNotFound)?;

        Ok(storage::get_cycle_penalty_pool(&env, group_id, cycle))
    }
}
