// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {BerylFees} from "./BerylFees.sol";

/// @title BerylTips
/// @notice Tips to creators in ETH or the one accepted token (Sepolia: Circle's test USDC),
///         paid straight from the tipper to the creator in the same call. The contract never
///         holds the money: ETH is forwarded at once and tokens move with transferFrom from
///         the tipper. Each tip carries `ref`, a hash of the Beryl post or profile (never
///         personal data), in its event.
/// @dev Test networks only (Sepolia). Mainnet use is a pause point: docs/contracts.md.
contract BerylTips is BerylFees, ReentrancyGuard {
    using SafeERC20 for IERC20;

    /// @notice The only token accepted. A standard ERC-20 (no fee on transfer, no rebasing).
    IERC20 public immutable token;

    /// @param token address(0) for ETH.
    /// @param amount What the tipper paid; the creator got `amount - fee`.
    event TipSent(
        address indexed tipper, address indexed creator, address indexed token, uint256 amount, uint256 fee, bytes32 ref
    );

    error ZeroAmount();
    error SelfTip();

    constructor(address owner_, address treasury_, IERC20 token_) BerylFees(owner_, treasury_) {
        if (address(token_) == address(0)) revert ZeroAddress();
        token = token_;
    }

    function tipEth(address creator, bytes32 ref) external payable nonReentrant {
        _check(creator, msg.value);
        (uint256 toCreator, uint256 fee) = quote(msg.value);
        emit TipSent(msg.sender, creator, address(0), msg.value, fee, ref);
        _sendEth(creator, toCreator);
        _sendEth(treasury, fee);
    }

    /// @notice Needs an allowance of `amount` for this contract first.
    function tipToken(address creator, uint256 amount, bytes32 ref) external nonReentrant {
        _check(creator, amount);
        (uint256 toCreator, uint256 fee) = quote(amount);
        emit TipSent(msg.sender, creator, address(token), amount, fee, ref);
        token.safeTransferFrom(msg.sender, creator, toCreator);
        if (fee > 0) token.safeTransferFrom(msg.sender, treasury, fee);
    }

    function _check(address creator, uint256 amount) private view {
        if (amount == 0) revert ZeroAmount();
        if (creator == address(0)) revert ZeroAddress();
        if (creator == msg.sender) revert SelfTip();
    }
}
