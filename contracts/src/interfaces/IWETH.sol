// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

/// @notice Wrapped ETH (WETH9): 1 WETH is always exactly 1 ETH.
interface IWETH {
    function deposit() external payable;
    function withdraw(uint256 amount) external;
}
