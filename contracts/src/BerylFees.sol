// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Ownable, Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";

/// @title BerylFees
/// @notice The optional protocol fee shared by BerylTips and BerylPaidPosts (phase W-2),
///         the same rule as the Solana program (solana/programs/beryl-tips/src/fees.rs):
///         basis points of each payment to `treasury`, 0 by default, never above
///         MAX_FEE_BPS (5%), set only by the owner. Ownership moves in two steps
///         (OpenZeppelin's Ownable2Step), so a typo can't hand it to a dead address.
/// @dev Test networks only (Sepolia). Mainnet use is a pause point: docs/contracts.md.
abstract contract BerylFees is Ownable2Step {
    /// @notice The hard cap: 5%. Even the owner can't set more.
    uint16 public constant MAX_FEE_BPS = 500;
    uint256 internal constant BPS = 10_000;

    uint16 public feeBps;
    address public treasury;

    event FeeSet(uint16 oldBps, uint16 newBps);
    event TreasurySet(address indexed oldTreasury, address indexed newTreasury);

    error FeeTooHigh();
    error ZeroAddress();
    error EthTransferFailed();

    constructor(address owner_, address treasury_) Ownable(owner_) {
        if (treasury_ == address(0)) revert ZeroAddress();
        treasury = treasury_;
        emit TreasurySet(address(0), treasury_);
    }

    function setFee(uint16 newBps) external onlyOwner {
        if (newBps > MAX_FEE_BPS) revert FeeTooHigh();
        emit FeeSet(feeBps, newBps);
        feeBps = newBps;
    }

    function setTreasury(address newTreasury) external onlyOwner {
        if (newTreasury == address(0)) revert ZeroAddress();
        emit TreasurySet(treasury, newTreasury);
        treasury = newTreasury;
    }

    /// @notice What `amount` pays the creator and the treasury now. The two always add up to
    ///         `amount`; the fee is rounded down, so the creator gets any fraction.
    function quote(uint256 amount) public view returns (uint256 toCreator, uint256 fee) {
        fee = amount * feeBps / BPS;
        toCreator = amount - fee;
    }

    /// @dev Sends ETH with a checked call; nothing is ever left in the contract.
    function _sendEth(address to, uint256 amount) internal {
        if (amount == 0) return;
        (bool ok,) = to.call{value: amount}("");
        if (!ok) revert EthTransferFailed();
    }
}
