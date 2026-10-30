// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BeryvaFactory} from "../../src/BeryvaFactory.sol";
import {BeryvaRouter} from "../../src/BeryvaRouter.sol";

/// @notice The real Sepolia WETH9 and Circle test USDC, on a fork: deploy, seed, swap both ways.
///         Skipped unless BERYVA_FORK_RPC is set (it needs the network):
///         BERYVA_FORK_RPC=https://ethereum-sepolia-rpc.publicnode.com forge test --match-path test/fork/*
contract SepoliaForkTest is Test {
    address constant WETH = 0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9;
    address constant USDC = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    BeryvaRouter router;
    address user = makeAddr("forkUser");

    function setUp() public {
        string memory rpc = vm.envOr("BERYVA_FORK_RPC", string(""));
        if (bytes(rpc).length == 0) {
            vm.skip(true);
            return;
        }
        vm.createSelectFork(rpc);
        assertEq(block.chainid, 11155111);
        BeryvaFactory factory = new BeryvaFactory();
        router = new BeryvaRouter(address(factory), WETH);
        vm.deal(user, 10 ether);
        deal(USDC, user, 10_000e6);
        vm.startPrank(user);
        IERC20(USDC).approve(address(router), type(uint256).max);
        router.addLiquidityETH{value: 1 ether}(USDC, 3_000e6, 0, 0, user, block.timestamp);
        vm.stopPrank();
    }

    function test_fork_swapBothWaysThroughRealWethAndUsdc() public {
        address[] memory toUsdc = new address[](2);
        (toUsdc[0], toUsdc[1]) = (WETH, USDC);
        address[] memory toEth = new address[](2);
        (toEth[0], toEth[1]) = (USDC, WETH);

        vm.startPrank(user);
        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256[] memory a = router.swapExactETHForTokens{value: 0.01 ether}(0, toUsdc, user, block.timestamp);
        assertEq(IERC20(USDC).balanceOf(user) - usdcBefore, a[1]);

        uint256 ethBefore = user.balance;
        uint256[] memory b = router.swapExactTokensForETH(a[1], 0, toEth, user, block.timestamp);
        assertEq(user.balance - ethBefore, b[1]);
        assertLt(b[1], 0.01 ether);
        vm.stopPrank();
        assertEq(address(router).balance, 0);
        assertEq(IERC20(WETH).balanceOf(address(router)), 0);
    }
}
