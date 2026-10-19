// Copyright 2026 Aventus DAO Ltd.
// SPDX-License-Identifier: GPL-3.0
//
// Aventus Node Manager, from https://github.com/AventusDAO/avn-parachain
// Added for PRDCTR on 2026-10-19.

//! `calculate_reward` must compute the node's exact rounded-down share of `total_reward`
//! (`weight * total_reward / total_weight`). Rounding the share to a fixed-point ratio first
//! and then multiplying loses precision whenever the denominator doesn't divide evenly, which
//! previously shorted recipients or zeroed out small rewards entirely.

#![cfg(test)]

use crate::tests::mock::*;
use sp_runtime::{ArithmeticError, DispatchError};

#[test]
fn rounds_down_to_the_exact_share_not_a_rounded_ratio() {
    // 1/3 of 300 is exactly 100. Rounding 1/3 to a fixed-point ratio first and then
    // multiplying by 300 gave 99.
    assert_eq!(NodeManager::calculate_reward(1, &3, &300u128), Ok(100));
}

#[test]
fn three_equal_shares_of_a_small_reward_do_not_all_round_to_zero() {
    // Three equal 1/3 shares of a reward of 3 must sum to exactly 3, not 0.
    let total_reward = 3u128;
    let paid: u128 = (0..3)
        .map(|_| NodeManager::calculate_reward(1, &3, &total_reward).unwrap())
        .sum();
    assert_eq!(paid, total_reward);
}

#[test]
fn zero_total_weight_is_a_division_by_zero_error() {
    assert_eq!(
        NodeManager::calculate_reward(1, &0, &100u128),
        Err(DispatchError::Arithmetic(ArithmeticError::DivisionByZero)),
    );
}

#[test]
fn overflow_returns_an_arithmetic_error_instead_of_panicking() {
    assert_eq!(
        NodeManager::calculate_reward(u128::MAX, &1, &u128::MAX),
        Err(DispatchError::Arithmetic(ArithmeticError::Overflow)),
    );
}
