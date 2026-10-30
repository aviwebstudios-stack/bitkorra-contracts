// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {BeryvaFactory} from "../../src/BeryvaFactory.sol";
import {BeryvaPair} from "../../src/BeryvaPair.sol";
import {BeryvaRouter} from "../../src/BeryvaRouter.sol";
import {MockERC20, MockWETH} from "../mocks/Mocks.sol";

/// @notice Random swaps both ways, deposits, withdrawals and attempts to take tokens
///         for free, by several people, with ghost variables that record any breach.
contract SwapHandler is Test {
    BeryvaRouter public router;
    BeryvaPair public pair;
    MockWETH public weth;
    MockERC20 public usdc;
    address[] public actors;

    bool public kDecreasedOnSwap;
    bool public freeSwapSucceeded;
    bool public shareValueDecreased;
    uint256 public swaps;
    uint256 public deposits;
    uint256 public withdrawals;
    uint256 public freeAttempts;

    uint256 private lastRootK;
    uint256 private lastSupply;

    constructor(BeryvaRouter router_, BeryvaPair pair_, MockWETH weth_, MockERC20 usdc_) {
        router = router_;
        pair = pair_;
        weth = weth_;
        usdc = usdc_;
        for (uint256 i; i < 4; i++) {
            address a = makeAddr(string(abi.encodePacked("actor", vm.toString(i))));
            actors.push(a);
            vm.deal(a, 1_000 ether);
            usdc.mint(a, 5_000_000e6);
            vm.prank(a);
            usdc.approve(address(router), type(uint256).max);
            vm.prank(a);
            pair.approve(address(router), type(uint256).max);
        }
        _snapshot();
    }

    function _k() private view returns (uint256) {
        (uint112 r0, uint112 r1) = pair.getReserves();
        return uint256(r0) * uint256(r1);
    }

    function _snapshot() private {
        lastRootK = Math.sqrt(_k());
        lastSupply = pair.totalSupply();
    }

    /// @dev The value of one LP share, sqrt(k) / supply, must never go down: fees raise it,
    ///      deposits and withdrawals round in the pool's favour. sqrt rounds down by less
    ///      than 1, so the check allows for that.
    function _checkShareValue() private {
        uint256 rootK = Math.sqrt(_k());
        uint256 supply = pair.totalSupply();
        if ((rootK + 1) * lastSupply < lastRootK * supply) shareValueDecreased = true;
        lastRootK = rootK;
        lastSupply = supply;
    }

    function _path(bool ethIn) private view returns (address[] memory p) {
        p = new address[](2);
        (p[0], p[1]) = ethIn ? (address(weth), address(usdc)) : (address(usdc), address(weth));
    }

    function swapEthForUsdc(uint256 actorSeed, uint256 amount) external {
        address a = actors[actorSeed % actors.length];
        amount = bound(amount, 1e9, 20 ether);
        uint256 kBefore = _k();
        vm.prank(a);
        try router.swapExactETHForTokens{value: amount}(0, _path(true), a, block.timestamp) {
            swaps++;
            if (_k() < kBefore) kDecreasedOnSwap = true;
        } catch {}
        _checkShareValue();
    }

    function swapUsdcForEth(uint256 actorSeed, uint256 amount) external {
        address a = actors[actorSeed % actors.length];
        amount = bound(amount, 1, 60_000e6);
        uint256 kBefore = _k();
        vm.prank(a);
        try router.swapExactTokensForETH(amount, 0, _path(false), a, block.timestamp) {
            swaps++;
            if (_k() < kBefore) kDecreasedOnSwap = true;
        } catch {}
        _checkShareValue();
    }

    function addLiquidity(uint256 actorSeed, uint256 ethAmount) external {
        address a = actors[actorSeed % actors.length];
        ethAmount = bound(ethAmount, 1e12, 10 ether);
        (uint112 r0, uint112 r1) = pair.getReserves();
        (uint256 rEth, uint256 rUsdc) = pair.token0() == address(weth) ? (uint256(r0), uint256(r1)) : (uint256(r1), uint256(r0));
        uint256 usdcAmount = (ethAmount * rUsdc) / rEth + 1;
        vm.prank(a);
        try router.addLiquidityETH{value: ethAmount}(address(usdc), usdcAmount, 0, 0, a, block.timestamp) {
            deposits++;
        } catch {}
        _checkShareValue();
    }

    function removeLiquidity(uint256 actorSeed, uint256 fraction) external {
        address a = actors[actorSeed % actors.length];
        uint256 owned = pair.balanceOf(a);
        if (owned == 0) return;
        uint256 amount = bound(fraction, 1, owned);
        vm.prank(a);
        try router.removeLiquidityETH(address(usdc), amount, 0, 0, a, block.timestamp) {
            withdrawals++;
        } catch {}
        _checkShareValue();
    }

    /// @notice Asks the pool for tokens while paying too little (or nothing).
    function tryFreeSwap(uint256 actorSeed, uint256 pay, uint256 ask) external {
        address a = actors[actorSeed % actors.length];
        (uint112 r0, uint112 r1) = pair.getReserves();
        bool wethIs0 = pair.token0() == address(weth);
        uint256 rEth = wethIs0 ? r0 : r1;
        uint256 rUsdc = wethIs0 ? r1 : r0;
        pay = bound(pay, 0, 1_000e6);
        // More than the formula gives for `pay`: (pay * 997 * rEth) / (rUsdc * 1000 + pay * 997) + 1 and up.
        uint256 fair = pay == 0 ? 0 : (pay * 997 * rEth) / (rUsdc * 1000 + pay * 997);
        if (fair + 1 >= rEth) return;
        ask = bound(ask, fair + 1, rEth - 1);
        freeAttempts++;
        if (pay > 0) {
            vm.prank(a);
            usdc.transfer(address(pair), pay);
        }
        vm.prank(a);
        try pair.swap(wethIs0 ? ask : 0, wethIs0 ? 0 : ask, a) {
            freeSwapSucceeded = true;
        } catch {}
        // Hand back any payment that stayed in the pool so later reserves stay comparable.
        pair.skim(a);
        _checkShareValue();
    }

    function warp(uint256 secondsAhead) external {
        vm.warp(block.timestamp + bound(secondsAhead, 1, 1 days));
    }
}

contract BeryvaSwapInvariantTest is Test {
    BeryvaFactory factory;
    BeryvaRouter router;
    MockWETH weth;
    MockERC20 usdc;
    BeryvaPair pair;
    SwapHandler handler;

    function setUp() public {
        weth = new MockWETH();
        usdc = new MockERC20("Test USDC", "USDC", 6);
        factory = new BeryvaFactory();
        router = new BeryvaRouter(address(factory), address(weth));

        address seeder = makeAddr("seeder");
        vm.deal(seeder, 10 ether);
        usdc.mint(seeder, 30_000e6);
        vm.startPrank(seeder);
        usdc.approve(address(router), type(uint256).max);
        router.addLiquidityETH{value: 10 ether}(address(usdc), 30_000e6, 0, 0, seeder, block.timestamp);
        vm.stopPrank();
        pair = BeryvaPair(factory.getPair(address(weth), address(usdc)));

        handler = new SwapHandler(router, pair, weth, usdc);
        targetContract(address(handler));
    }

    function invariant_kNeverDecreasesOnASwap() public view {
        assertFalse(handler.kDecreasedOnSwap());
    }

    function invariant_noFreeTokens() public view {
        assertFalse(handler.freeSwapSucceeded());
    }

    function invariant_lpShareValueNeverDecreases() public view {
        assertFalse(handler.shareValueDecreased());
    }

    function invariant_reservesMatchBalances() public view {
        (uint112 r0, uint112 r1) = pair.getReserves();
        assertEq(MockERC20(pair.token0()).balanceOf(address(pair)), r0);
        assertEq(MockERC20(pair.token1()).balanceOf(address(pair)), r1);
    }

    function invariant_routerHoldsNothing() public view {
        assertEq(address(router).balance, 0);
        assertEq(weth.balanceOf(address(router)), 0);
        assertEq(usdc.balanceOf(address(router)), 0);
        assertEq(pair.balanceOf(address(router)), 0);
    }

    function invariant_minimumLiquidityStaysLocked() public view {
        assertEq(pair.balanceOf(pair.DEAD()), pair.MINIMUM_LIQUIDITY());
        assertGt(pair.totalSupply(), 0);
    }

    function invariant_wethIsFullyBacked() public view {
        assertEq(address(weth).balance, weth.totalSupply());
    }

    function invariant_callSummary() public view {
        // Not a check: keeps the counters in the report (forge test -vv).
        handler.swaps();
        handler.deposits();
        handler.withdrawals();
        handler.freeAttempts();
    }
}
