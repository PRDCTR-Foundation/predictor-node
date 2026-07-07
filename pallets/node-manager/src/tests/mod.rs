// Copyright 2026 Aventus DAO.

//! Test suite for the node-manager pallet.
//!
//! `mock` provides the test runtime; the remaining modules group tests by area.

mod mock;

mod test_admin;
mod test_delegated_heartbeat;
mod test_heartbeat;
mod test_node_deregistration;
mod test_node_registration;
mod test_on_idle_drain;
mod test_reward_halving;
mod test_top_up_reward_pot;
