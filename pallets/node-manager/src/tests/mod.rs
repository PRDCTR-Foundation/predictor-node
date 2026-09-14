// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-07-07.

//! Test suite for the node-manager pallet.
//!
//! `mock` provides the test runtime; the remaining modules group tests by area.

pub(crate) mod mock;

mod test_admin;
mod test_delegated_heartbeat;
mod test_heartbeat;
mod test_next_reward_amount;
mod test_node_deregistration;
mod test_node_registration;
mod test_on_idle_drain;
mod test_reward_halving;
mod test_reward_lock;
mod test_top_up_reward_pot;
