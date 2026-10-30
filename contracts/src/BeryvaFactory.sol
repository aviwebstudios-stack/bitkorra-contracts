// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {BeryvaPair} from "./BeryvaPair.sol";

/// @title Beryva Swap factory
/// @notice Creates one pool per pair of tokens and keeps the list. Anyone can create a pool;
///         there is no owner, no fee switch and no way to change a pool once it exists.
/// @dev Pools are deployed with CREATE2 (salt: the sorted token addresses), so a pool's
///      address is fixed by its tokens. Test networks only: docs/contracts.md.
contract BeryvaFactory {
    mapping(address => mapping(address => address)) public getPair;
    address[] public allPairs;

    event PairCreated(address indexed token0, address indexed token1, address pair, uint256 index);

    error IdenticalAddresses();
    error ZeroAddress();
    error PairExists();

    function allPairsLength() external view returns (uint256) {
        return allPairs.length;
    }

    function createPair(address tokenA, address tokenB) external returns (address pair) {
        if (tokenA == tokenB) revert IdenticalAddresses();
        (address token0, address token1) = tokenA < tokenB ? (tokenA, tokenB) : (tokenB, tokenA);
        if (token0 == address(0)) revert ZeroAddress();
        if (getPair[token0][token1] != address(0)) revert PairExists();

        pair = address(new BeryvaPair{salt: keccak256(abi.encodePacked(token0, token1))}(token0, token1));
        getPair[token0][token1] = pair;
        getPair[token1][token0] = pair;
        allPairs.push(pair);
        emit PairCreated(token0, token1, pair, allPairs.length);
    }
}
