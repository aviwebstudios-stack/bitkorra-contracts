// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {BeryvaFactory} from "../src/BeryvaFactory.sol";
import {BeryvaPair} from "../src/BeryvaPair.sol";
import {BeryvaRouter} from "../src/BeryvaRouter.sol";
import {BeryvaLibrary} from "../src/BeryvaLibrary.sol";
import {MockERC20, MockWETH, ReentrantReceiver, EthRefuser} from "./mocks/Mocks.sol";

/// @dev Exposes the library's internal maths so tests can call it.
contract LibraryHarness {
    function getAmountOut(uint256 a, uint256 rIn, uint256 rOut) external pure returns (uint256) {
        return BeryvaLibrary.getAmountOut(a, rIn, rOut);
    }

    function getAmountIn(uint256 a, uint256 rIn, uint256 rOut) external pure returns (uint256) {
        return BeryvaLibrary.getAmountIn(a, rIn, rOut);
    }

    function quote(uint256 a, uint256 rA, uint256 rB) external pure returns (uint256) {
        return BeryvaLibrary.quote(a, rA, rB);
    }
}

contract BeryvaSwapTest is Test {
    MockWETH weth;
    MockERC20 usdc;
    MockERC20 dai;
    BeryvaFactory factory;
    BeryvaRouter router;
    LibraryHarness lib;

    address lp = makeAddr("lp");
    address alice = makeAddr("alice");
    address bob = makeAddr("bob");

    uint256 constant SEED_ETH = 10 ether;
    uint256 constant SEED_USDC = 30_000e6;

    function setUp() public {
        weth = new MockWETH();
        usdc = new MockERC20("Test USDC", "USDC", 6);
        dai = new MockERC20("Test DAI", "DAI", 18);
        factory = new BeryvaFactory();
        router = new BeryvaRouter(address(factory), address(weth));
        lib = new LibraryHarness();

        vm.deal(lp, 1_000 ether);
        usdc.mint(lp, 10_000_000e6);
        dai.mint(lp, 10_000_000e18);
        vm.startPrank(lp);
        usdc.approve(address(router), type(uint256).max);
        dai.approve(address(router), type(uint256).max);
        router.addLiquidityETH{value: SEED_ETH}(address(usdc), SEED_USDC, 0, 0, lp, block.timestamp);
        vm.stopPrank();

        vm.deal(alice, 100 ether);
        usdc.mint(alice, 100_000e6);
    }

    function pair() internal view returns (BeryvaPair) {
        return BeryvaPair(factory.getPair(address(weth), address(usdc)));
    }

    function ethToUsdcPath() internal view returns (address[] memory p) {
        p = new address[](2);
        (p[0], p[1]) = (address(weth), address(usdc));
    }

    function usdcToEthPath() internal view returns (address[] memory p) {
        p = new address[](2);
        (p[0], p[1]) = (address(usdc), address(weth));
    }

    /// @return rEth The pool's WETH reserve. @return rUsdc The pool's USDC reserve.
    function reserves() internal view returns (uint256 rEth, uint256 rUsdc) {
        (uint112 r0, uint112 r1) = pair().getReserves();
        (rEth, rUsdc) = pair().token0() == address(weth) ? (uint256(r0), uint256(r1)) : (uint256(r1), uint256(r0));
    }

    // ---------------------------------------------------------------------
    // Factory

    function test_factory_createsSortedPairOnce() public {
        address p = factory.createPair(address(dai), address(usdc));
        BeryvaPair created = BeryvaPair(p);
        (address t0, address t1) = address(dai) < address(usdc) ? (address(dai), address(usdc)) : (address(usdc), address(dai));
        assertEq(created.token0(), t0);
        assertEq(created.token1(), t1);
        assertEq(created.factory(), address(factory));
        assertEq(factory.getPair(address(dai), address(usdc)), p);
        assertEq(factory.getPair(address(usdc), address(dai)), p);
        assertEq(factory.allPairsLength(), 2);

        vm.expectRevert(BeryvaFactory.PairExists.selector);
        factory.createPair(address(usdc), address(dai));
    }

    function test_factory_rejectsBadTokens() public {
        vm.expectRevert(BeryvaFactory.IdenticalAddresses.selector);
        factory.createPair(address(usdc), address(usdc));
        vm.expectRevert(BeryvaFactory.ZeroAddress.selector);
        factory.createPair(address(0), address(usdc));
    }

    function test_factory_pairAddressIsCreate2() public {
        (address t0, address t1) = address(dai) < address(usdc) ? (address(dai), address(usdc)) : (address(usdc), address(dai));
        bytes32 salt = keccak256(abi.encodePacked(t0, t1));
        bytes32 initHash = keccak256(abi.encodePacked(type(BeryvaPair).creationCode, abi.encode(t0, t1)));
        address expected = vm.computeCreate2Address(salt, initHash, address(factory));
        assertEq(factory.createPair(address(usdc), address(dai)), expected);
    }

    // ---------------------------------------------------------------------
    // Pool: liquidity

    function test_firstDeposit_locksMinimumLiquidity() public view {
        BeryvaPair p = pair();
        uint256 root = Math.sqrt(SEED_ETH * SEED_USDC);
        assertEq(p.balanceOf(p.DEAD()), p.MINIMUM_LIQUIDITY());
        assertEq(p.balanceOf(lp), root - p.MINIMUM_LIQUIDITY());
        assertEq(p.totalSupply(), root);
        (uint256 rEth, uint256 rUsdc) = reserves();
        assertEq(rEth, SEED_ETH);
        assertEq(rUsdc, SEED_USDC);
    }

    function test_firstDeposit_tooSmallReverts() public {
        address p = factory.createPair(address(dai), address(usdc));
        vm.startPrank(lp);
        dai.transfer(p, 1000);
        usdc.transfer(p, 1000);
        vm.expectRevert(BeryvaPair.InsufficientLiquidityMinted.selector);
        BeryvaPair(p).mint(lp);
        vm.stopPrank();
    }

    function test_addLiquidity_usesPoolRatioAndRefundsExtraEth() public {
        uint256 before = alice.balance;
        vm.startPrank(alice);
        usdc.approve(address(router), type(uint256).max);
        // 3,000 USDC matches 1 ETH at 10:30,000; 2 ETH is sent, 1 must come back.
        (uint256 amountToken, uint256 amountETH, uint256 liquidity) = router.addLiquidityETH{value: 2 ether}(address(usdc), 3_000e6, 0, 0, alice, block.timestamp);
        vm.stopPrank();
        assertEq(amountToken, 3_000e6);
        assertEq(amountETH, 1 ether);
        assertEq(before - alice.balance, 1 ether);
        assertEq(liquidity, pair().balanceOf(alice));
        assertEq(address(router).balance, 0);
    }

    function test_addLiquidity_minimumsAreEnforced() public {
        vm.startPrank(alice);
        usdc.approve(address(router), type(uint256).max);
        // 1 ETH only matches 3,000 USDC: asking for at least 3,001 USDC to go in must revert.
        vm.expectRevert(BeryvaRouter.InsufficientAAmount.selector);
        router.addLiquidityETH{value: 1 ether}(address(usdc), 4_000e6, 3_001e6, 0, alice, block.timestamp);
        vm.expectRevert(BeryvaRouter.InsufficientBAmount.selector);
        router.addLiquidityETH{value: 2 ether}(address(usdc), 3_000e6, 0, 1.5 ether, alice, block.timestamp);
        vm.stopPrank();
    }

    function test_removeLiquidityETH_returnsShareRoundedDown() public {
        BeryvaPair p = pair();
        uint256 share = p.balanceOf(lp) / 4;
        (uint256 rEth, uint256 rUsdc) = reserves();
        uint256 supply = p.totalSupply();
        uint256 ethBefore = lp.balance;
        uint256 usdcBefore = usdc.balanceOf(lp);
        vm.startPrank(lp);
        p.approve(address(router), share);
        (uint256 amountToken, uint256 amountETH) = router.removeLiquidityETH(address(usdc), share, 0, 0, lp, block.timestamp);
        vm.stopPrank();
        assertEq(amountETH, (share * rEth) / supply);
        assertEq(amountToken, (share * rUsdc) / supply);
        assertEq(lp.balance - ethBefore, amountETH);
        assertEq(usdc.balanceOf(lp) - usdcBefore, amountToken);
    }

    function test_removeLiquidity_minimumsAreEnforced() public {
        BeryvaPair p = pair();
        uint256 share = p.balanceOf(lp) / 4;
        vm.startPrank(lp);
        p.approve(address(router), share);
        vm.expectRevert(BeryvaRouter.InsufficientAAmount.selector);
        router.removeLiquidityETH(address(usdc), share, 10_000e6, 0, lp, block.timestamp);
        vm.stopPrank();
    }

    // ---------------------------------------------------------------------
    // Pool: swaps called directly

    function test_pairSwap_withoutInputReverts() public {
        BeryvaPair p = pair();
        bool usdcIs0 = p.token0() == address(usdc);
        vm.expectRevert(BeryvaPair.InsufficientInputAmount.selector);
        p.swap(usdcIs0 ? 1e6 : 0, usdcIs0 ? 0 : 1e6, alice);
    }

    function test_pairSwap_exactQuotePassesAndOneMoreReverts() public {
        BeryvaPair p = pair();
        (uint256 rEth, uint256 rUsdc) = reserves();
        uint256 amountIn = 1 ether;
        uint256 out = lib.getAmountOut(amountIn, rEth, rUsdc);
        bool usdcIs0 = p.token0() == address(usdc);

        vm.startPrank(alice);
        weth.deposit{value: 2 ether}();
        weth.transfer(address(p), amountIn);
        vm.expectRevert(BeryvaPair.KInvariant.selector);
        p.swap(usdcIs0 ? out + 1 : 0, usdcIs0 ? 0 : out + 1, alice);
        p.swap(usdcIs0 ? out : 0, usdcIs0 ? 0 : out, alice);
        vm.stopPrank();
        assertEq(usdc.balanceOf(alice), 100_000e6 + out);
    }

    function test_pairSwap_rejectsBadOutputs() public {
        BeryvaPair p = pair();
        vm.expectRevert(BeryvaPair.InsufficientOutputAmount.selector);
        p.swap(0, 0, alice);
        (uint112 r0,) = p.getReserves();
        vm.expectRevert(BeryvaPair.InsufficientLiquidity.selector);
        p.swap(r0, 0, alice);
        vm.expectRevert(BeryvaPair.InvalidTo.selector);
        p.swap(1, 0, address(usdc));
        vm.expectRevert(BeryvaPair.InvalidTo.selector);
        p.swap(1, 0, address(p));
    }

    function test_skimAndSync() public {
        BeryvaPair p = pair();
        vm.prank(alice);
        usdc.transfer(address(p), 5e6);
        p.skim(bob);
        assertEq(usdc.balanceOf(bob), 5e6);

        vm.prank(alice);
        usdc.transfer(address(p), 7e6);
        p.sync();
        (, uint256 rUsdc) = reserves();
        assertEq(rUsdc, SEED_USDC + 7e6);
    }

    // ---------------------------------------------------------------------
    // Router: swaps

    function test_swapExactETHForTokens_paysTheQuote() public {
        uint256[] memory quoted = router.getAmountsOut(1 ether, ethToUsdcPath());
        vm.prank(alice);
        uint256[] memory amounts = router.swapExactETHForTokens{value: 1 ether}(quoted[1], ethToUsdcPath(), alice, block.timestamp);
        assertEq(amounts[1], quoted[1]);
        assertEq(usdc.balanceOf(alice), 100_000e6 + quoted[1]);
        // 10 ETH : 30,000 USDC, 1 ETH in: 997 * 30000e6 / (10000 + 997) rounded down.
        assertEq(quoted[1], 2_719_832_681);
        assertEq(address(router).balance, 0);
        assertEq(weth.balanceOf(address(router)), 0);
    }

    function test_swapExactTokensForETH_paysTheQuote() public {
        uint256[] memory quoted = router.getAmountsOut(3_000e6, usdcToEthPath());
        uint256 before = alice.balance;
        vm.startPrank(alice);
        usdc.approve(address(router), 3_000e6);
        router.swapExactTokensForETH(3_000e6, quoted[1], usdcToEthPath(), alice, block.timestamp);
        vm.stopPrank();
        assertEq(alice.balance - before, quoted[1]);
        assertEq(usdc.allowance(alice, address(router)), 0);
    }

    function test_swap_slippageLimitReverts() public {
        uint256[] memory quoted = router.getAmountsOut(1 ether, ethToUsdcPath());
        vm.prank(alice);
        vm.expectRevert(BeryvaRouter.InsufficientOutputAmount.selector);
        router.swapExactETHForTokens{value: 1 ether}(quoted[1] + 1, ethToUsdcPath(), alice, block.timestamp);
    }

    function test_swap_priceMovedBeforeItRuns() public {
        // Alice's minimum comes from a quote; someone else swaps first and moves the price.
        uint256[] memory quoted = router.getAmountsOut(1 ether, ethToUsdcPath());
        uint256 minOut = (quoted[1] * 9950) / 10_000; // 0.5% slippage
        vm.deal(bob, 10 ether);
        vm.prank(bob);
        router.swapExactETHForTokens{value: 1 ether}(0, ethToUsdcPath(), bob, block.timestamp);
        vm.prank(alice);
        vm.expectRevert(BeryvaRouter.InsufficientOutputAmount.selector);
        router.swapExactETHForTokens{value: 1 ether}(minOut, ethToUsdcPath(), alice, block.timestamp);
    }

    function test_swap_deadlinePassedReverts() public {
        vm.warp(1_000_000);
        vm.prank(alice);
        vm.expectRevert(BeryvaRouter.Expired.selector);
        router.swapExactETHForTokens{value: 1 ether}(0, ethToUsdcPath(), alice, block.timestamp - 1);
    }

    function test_swap_needsAllowance() public {
        vm.prank(alice);
        vm.expectRevert();
        router.swapExactTokensForETH(1e6, 0, usdcToEthPath(), alice, block.timestamp);
    }

    function test_swap_rejectsWrongPathsAndRecipients() public {
        vm.startPrank(alice);
        usdc.approve(address(router), type(uint256).max);
        vm.expectRevert(BeryvaRouter.InvalidPath.selector);
        router.swapExactETHForTokens{value: 1 ether}(0, usdcToEthPath(), alice, block.timestamp);
        vm.expectRevert(BeryvaRouter.InvalidPath.selector);
        router.swapExactTokensForETH(1e6, 0, ethToUsdcPath(), alice, block.timestamp);
        vm.expectRevert(BeryvaRouter.ZeroAddress.selector);
        router.swapExactTokensForETH(1e6, 0, usdcToEthPath(), address(0), block.timestamp);
        vm.expectRevert(BeryvaRouter.ZeroAddress.selector);
        router.swapExactETHForTokens{value: 1 ether}(0, ethToUsdcPath(), address(0), block.timestamp);
        address[] memory one = new address[](1);
        one[0] = address(weth);
        vm.expectRevert(BeryvaLibrary.InvalidPath.selector);
        router.swapExactETHForTokens{value: 1 ether}(0, one, alice, block.timestamp);
        vm.stopPrank();
    }

    function test_swap_unknownPairReverts() public {
        address[] memory path = new address[](2);
        (path[0], path[1]) = (address(weth), address(dai));
        vm.prank(alice);
        vm.expectRevert(BeryvaLibrary.PairNotFound.selector);
        router.swapExactETHForTokens{value: 1 ether}(0, path, alice, block.timestamp);
    }

    function test_router_onlyTakesEthFromWeth() public {
        vm.prank(alice);
        (bool ok,) = address(router).call{value: 1 ether}("");
        assertFalse(ok);
    }

    function test_swapToEth_recipientThatRefusesEthReverts() public {
        EthRefuser refuser = new EthRefuser();
        vm.startPrank(alice);
        usdc.approve(address(router), 1e6);
        vm.expectRevert(BeryvaRouter.EthTransferFailed.selector);
        router.swapExactTokensForETH(1e6, 0, usdcToEthPath(), address(refuser), block.timestamp);
        vm.stopPrank();
    }

    function test_swapToEth_reentryIsBlocked() public {
        ReentrantReceiver attacker = new ReentrantReceiver(address(router));
        vm.startPrank(alice);
        usdc.approve(address(router), 100e6);
        router.swapExactTokensForETH(100e6, 0, usdcToEthPath(), address(attacker), block.timestamp);
        vm.stopPrank();
        assertTrue(attacker.reentered());
        assertTrue(attacker.reentryBlocked());
        assertGt(address(attacker).balance, 0);
    }

    function test_multiHop_usdcToEthToDai() public {
        vm.prank(lp);
        router.addLiquidityETH{value: 10 ether}(address(dai), 30_000e18, 0, 0, lp, block.timestamp);
        address[] memory path = new address[](3);
        (path[0], path[1], path[2]) = (address(usdc), address(weth), address(dai));
        uint256[] memory quoted = router.getAmountsOut(1_000e6, path);
        vm.startPrank(alice);
        usdc.approve(address(router), 1_000e6);
        router.swapExactTokensForTokens(1_000e6, quoted[2], path, alice, block.timestamp);
        vm.stopPrank();
        assertEq(dai.balanceOf(alice), quoted[2]);
    }

    function test_feeStaysInThePool() public {
        (uint256 rEth0, uint256 rUsdc0) = reserves();
        vm.prank(alice);
        router.swapExactETHForTokens{value: 1 ether}(0, ethToUsdcPath(), alice, block.timestamp);
        (uint256 rEth1, uint256 rUsdc1) = reserves();
        assertGt(rEth1 * rUsdc1, rEth0 * rUsdc0);
    }

    // ---------------------------------------------------------------------
    // Quote vectors, shared with the app (tests/beryva/swap-amm.test.ts)

    function test_quoteVectors() public view {
        assertEq(lib.getAmountOut(1e18, 10e18, 30_000e6), 2_719_832_681);
        assertEq(lib.getAmountOut(3_000e6, 30_000e6, 10e18), 906_610_893_880_149_131);
        assertEq(lib.getAmountOut(1, 1_000_000, 1_000_000), 0);
        assertEq(lib.getAmountOut(5e15, 1e16, 25e6), 8_316_649);
        assertEq(lib.getAmountIn(1_000e6, 10e18, 30_000e6), 345_865_181_752_153_011);
        assertEq(lib.quote(1e18, 10e18, 30_000e6), 3_000e6);
    }

    function test_libraryRejectsEmptyInputs() public {
        vm.expectRevert(BeryvaLibrary.InsufficientAmount.selector);
        lib.getAmountOut(0, 1, 1);
        vm.expectRevert(BeryvaLibrary.InsufficientLiquidity.selector);
        lib.getAmountOut(1, 0, 1);
        vm.expectRevert(BeryvaLibrary.InsufficientLiquidity.selector);
        lib.getAmountIn(10, 100, 10);
    }

    // ---------------------------------------------------------------------
    // Fuzz

    function testFuzz_swapPaysExactlyTheQuote(uint256 amountIn) public {
        amountIn = bound(amountIn, 1e9, 50 ether);
        vm.deal(alice, amountIn);
        uint256[] memory quoted = router.getAmountsOut(amountIn, ethToUsdcPath());
        vm.assume(quoted[1] > 0);
        uint256 before = usdc.balanceOf(alice);
        vm.prank(alice);
        router.swapExactETHForTokens{value: amountIn}(quoted[1], ethToUsdcPath(), alice, block.timestamp);
        assertEq(usdc.balanceOf(alice) - before, quoted[1]);
    }

    function testFuzz_kNeverDecreases(uint256 amountIn, bool ethIn) public {
        (uint256 rEth0, uint256 rUsdc0) = reserves();
        vm.startPrank(alice);
        if (ethIn) {
            amountIn = bound(amountIn, 1e9, 100 ether);
            vm.deal(alice, amountIn);
            router.swapExactETHForTokens{value: amountIn}(0, ethToUsdcPath(), alice, block.timestamp);
        } else {
            amountIn = bound(amountIn, 1, 100_000e6);
            usdc.approve(address(router), amountIn);
            uint256[] memory q = router.getAmountsOut(amountIn, usdcToEthPath());
            vm.assume(q[1] > 0);
            router.swapExactTokensForETH(amountIn, 0, usdcToEthPath(), alice, block.timestamp);
        }
        vm.stopPrank();
        (uint256 rEth1, uint256 rUsdc1) = reserves();
        assertGe(rEth1 * rUsdc1, rEth0 * rUsdc0);
    }

    function testFuzz_roundTripNeverProfits(uint256 amountIn) public {
        amountIn = bound(amountIn, 1e12, 20 ether);
        vm.deal(bob, amountIn);
        vm.startPrank(bob);
        uint256[] memory a = router.swapExactETHForTokens{value: amountIn}(0, ethToUsdcPath(), bob, block.timestamp);
        usdc.approve(address(router), a[1]);
        uint256[] memory q = router.getAmountsOut(a[1], usdcToEthPath());
        vm.assume(q[1] > 0);
        router.swapExactTokensForETH(a[1], 0, usdcToEthPath(), bob, block.timestamp);
        vm.stopPrank();
        assertLt(bob.balance, amountIn);
    }

    function testFuzz_oneMoreThanTheQuoteAlwaysReverts(uint256 amountIn) public {
        amountIn = bound(amountIn, 1, 100_000e6);
        BeryvaPair p = pair();
        (uint256 rEth, uint256 rUsdc) = reserves();
        uint256 out = lib.getAmountOut(amountIn, rUsdc, rEth);
        bool wethIs0 = p.token0() == address(weth);
        vm.startPrank(alice);
        usdc.transfer(address(p), amountIn);
        vm.expectRevert();
        p.swap(wethIs0 ? out + 1 : 0, wethIs0 ? 0 : out + 1, alice);
        vm.stopPrank();
    }

    function testFuzz_getAmountInIsEnough(uint256 amountOut) public view {
        (uint256 rEth, uint256 rUsdc) = reserves();
        amountOut = bound(amountOut, 1, rUsdc - 1);
        uint256 amountIn = lib.getAmountIn(amountOut, rEth, rUsdc);
        assertGe(lib.getAmountOut(amountIn, rEth, rUsdc), amountOut);
    }

    function testFuzz_depositThenWithdrawNeverProfits(uint256 ethIn) public {
        ethIn = bound(ethIn, 1e12, 50 ether);
        (uint256 rEth, uint256 rUsdc) = reserves();
        uint256 usdcIn = (ethIn * rUsdc) / rEth + 1;
        vm.deal(bob, ethIn);
        usdc.mint(bob, usdcIn);
        vm.startPrank(bob);
        usdc.approve(address(router), usdcIn);
        (uint256 tokenUsed, uint256 ethUsed, uint256 liquidity) = router.addLiquidityETH{value: ethIn}(address(usdc), usdcIn, 0, 0, bob, block.timestamp);
        BeryvaPair p = pair();
        p.approve(address(router), liquidity);
        (uint256 tokenBack, uint256 ethBack) = router.removeLiquidityETH(address(usdc), liquidity, 0, 0, bob, block.timestamp);
        vm.stopPrank();
        assertLe(tokenBack, tokenUsed);
        assertLe(ethBack, ethUsed);
    }

    function testFuzz_donationsCannotBeStolenBySwap(uint256 donation, uint256 ask) public {
        // Tokens sent to the pool without a swap count as input for whoever swaps next,
        // but never more than the formula allows for that input.
        donation = bound(donation, 1, 10_000e6);
        BeryvaPair p = pair();
        (uint256 rEth, uint256 rUsdc) = reserves();
        uint256 maxOut = lib.getAmountOut(donation, rUsdc, rEth);
        ask = bound(ask, maxOut + 1, rEth - 1);
        vm.prank(alice);
        usdc.transfer(address(p), donation);
        bool wethIs0 = p.token0() == address(weth);
        vm.prank(bob);
        vm.expectRevert(BeryvaPair.KInvariant.selector);
        p.swap(wethIs0 ? ask : 0, wethIs0 ? 0 : ask, bob);
    }
}
