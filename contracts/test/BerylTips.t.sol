// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {IERC20Errors} from "@openzeppelin/contracts/interfaces/draft-IERC6093.sol";
import {BerylFees} from "../src/BerylFees.sol";
import {BerylTips} from "../src/BerylTips.sol";
import {MockERC20, RejectingCreator, ReentrantCreator} from "./mocks/Mocks.sol";

/// @notice Phase W-2: tips in ETH and test USDC, the fee and its cap, Ownable2Step.
contract BerylTipsTest is Test {
    BerylTips tips;
    MockERC20 usdc;
    address owner = makeAddr("owner");
    address treasury = makeAddr("treasury");
    address tipper = makeAddr("tipper");
    address creator = makeAddr("creator");
    bytes32 constant REF = keccak256("beryl:post:abc");

    event TipSent(
        address indexed tipper, address indexed creator, address indexed token, uint256 amount, uint256 fee, bytes32 ref
    );

    function setUp() public {
        usdc = new MockERC20("USD Coin", "USDC", 6);
        tips = new BerylTips(owner, treasury, usdc);
        vm.deal(tipper, 100 ether);
        usdc.mint(tipper, 1_000_000e6);
        vm.prank(tipper);
        usdc.approve(address(tips), type(uint256).max);
    }

    function _fee(uint16 bps) internal {
        vm.prank(owner);
        tips.setFee(bps);
    }

    // -- deployment ---------------------------------------------------------

    function test_deploys_with_no_fee_and_refuses_zero_addresses() public {
        assertEq(tips.feeBps(), 0);
        assertEq(tips.owner(), owner);
        assertEq(tips.treasury(), treasury);
        assertEq(address(tips.token()), address(usdc));
        vm.expectRevert(BerylFees.ZeroAddress.selector);
        new BerylTips(owner, address(0), usdc);
        vm.expectRevert(BerylFees.ZeroAddress.selector);
        new BerylTips(owner, treasury, MockERC20(address(0)));
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableInvalidOwner.selector, address(0)));
        new BerylTips(address(0), treasury, usdc);
    }

    // -- ETH tips -----------------------------------------------------------

    function test_eth_tip_goes_straight_to_the_creator() public {
        vm.expectEmit(address(tips));
        emit TipSent(tipper, creator, address(0), 0.1 ether, 0, REF);
        vm.prank(tipper);
        tips.tipEth{value: 0.1 ether}(creator, REF);
        assertEq(creator.balance, 0.1 ether);
        assertEq(treasury.balance, 0);
        assertEq(address(tips).balance, 0);
    }

    function test_eth_tip_with_fee_splits_and_rounds_for_the_creator() public {
        _fee(250);
        vm.prank(tipper);
        tips.tipEth{value: 1_000_003}(creator, REF);
        assertEq(treasury.balance, 25_000);
        assertEq(creator.balance, 975_003);
        assertEq(address(tips).balance, 0);
    }

    function test_eth_tip_refusals() public {
        vm.startPrank(tipper);
        vm.expectRevert(BerylTips.ZeroAmount.selector);
        tips.tipEth{value: 0}(creator, REF);
        vm.expectRevert(BerylTips.SelfTip.selector);
        tips.tipEth{value: 1}(tipper, REF);
        vm.expectRevert(BerylFees.ZeroAddress.selector);
        tips.tipEth{value: 1}(address(0), REF);
        vm.stopPrank();
    }

    function test_a_creator_that_refuses_eth_makes_the_tip_revert_and_nothing_is_stuck() public {
        RejectingCreator rc = new RejectingCreator();
        vm.prank(tipper);
        vm.expectRevert(BerylFees.EthTransferFailed.selector);
        tips.tipEth{value: 1 ether}(address(rc), REF);
        assertEq(address(tips).balance, 0);
        assertEq(tipper.balance, 100 ether);
    }

    function test_reentering_from_the_creator_is_blocked() public {
        ReentrantCreator rc = new ReentrantCreator();
        rc.arm(address(tips), abi.encodeCall(BerylTips.tipEth, (creator, REF)));
        vm.prank(tipper);
        tips.tipEth{value: 1 ether}(address(rc), REF);
        assertTrue(rc.reentered());
        assertTrue(rc.reentryBlocked());
        assertEq(address(rc).balance, 1 ether);
        assertEq(creator.balance, 0);
        assertEq(address(tips).balance, 0);
    }

    function test_plain_eth_sent_to_the_contract_is_refused() public {
        vm.prank(tipper);
        (bool ok,) = address(tips).call{value: 1 ether}("");
        assertFalse(ok);
        assertEq(address(tips).balance, 0);
    }

    // -- token tips ---------------------------------------------------------

    function test_token_tip_goes_straight_to_the_creator() public {
        vm.expectEmit(address(tips));
        emit TipSent(tipper, creator, address(usdc), 5e6, 0, REF);
        vm.prank(tipper);
        tips.tipToken(creator, 5e6, REF);
        assertEq(usdc.balanceOf(creator), 5e6);
        assertEq(usdc.balanceOf(address(tips)), 0);
    }

    function test_token_tip_with_fee() public {
        _fee(500);
        vm.prank(tipper);
        tips.tipToken(creator, 10e6, REF);
        assertEq(usdc.balanceOf(creator), 9.5e6);
        assertEq(usdc.balanceOf(treasury), 0.5e6);
        assertEq(usdc.balanceOf(address(tips)), 0);
    }

    function test_token_tip_refusals() public {
        vm.startPrank(tipper);
        vm.expectRevert(BerylTips.ZeroAmount.selector);
        tips.tipToken(creator, 0, REF);
        vm.expectRevert(BerylTips.SelfTip.selector);
        tips.tipToken(tipper, 1, REF);
        vm.stopPrank();
        // No allowance.
        address stranger = makeAddr("stranger");
        usdc.mint(stranger, 10e6);
        vm.prank(stranger);
        vm.expectRevert(
            abi.encodeWithSelector(IERC20Errors.ERC20InsufficientAllowance.selector, address(tips), 0, 1e6)
        );
        tips.tipToken(creator, 1e6, REF);
    }

    // -- fee and ownership --------------------------------------------------

    function test_only_the_owner_sets_the_fee_and_never_above_5_percent() public {
        vm.prank(tipper);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, tipper));
        tips.setFee(100);
        vm.startPrank(owner);
        vm.expectRevert(BerylFees.FeeTooHigh.selector);
        tips.setFee(501);
        tips.setFee(500);
        assertEq(tips.feeBps(), 500);
        vm.expectRevert(BerylFees.ZeroAddress.selector);
        tips.setTreasury(address(0));
        tips.setTreasury(creator);
        assertEq(tips.treasury(), creator);
        vm.stopPrank();
    }

    function test_ownership_moves_in_two_steps() public {
        address next = makeAddr("next");
        vm.prank(owner);
        tips.transferOwnership(next);
        assertEq(tips.owner(), owner);
        assertEq(tips.pendingOwner(), next);
        vm.prank(tipper);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, tipper));
        tips.acceptOwnership();
        vm.prank(next);
        tips.acceptOwnership();
        assertEq(tips.owner(), next);
        vm.prank(owner);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, owner));
        tips.setFee(1);
    }

    // -- fuzz -----------------------------------------------------------------

    function testFuzz_eth_tip_pays_creator_amount_minus_fee(uint96 amount, uint16 bps) public {
        amount = uint96(bound(amount, 1, 100 ether));
        bps = uint16(bound(bps, 0, 500));
        _fee(bps);
        vm.prank(tipper);
        tips.tipEth{value: amount}(creator, REF);
        uint256 fee = uint256(amount) * bps / 10_000;
        assertEq(treasury.balance, fee);
        assertEq(creator.balance, amount - fee);
        assertEq(address(tips).balance, 0);
    }

    function testFuzz_token_tip_pays_creator_amount_minus_fee(uint256 amount, uint16 bps) public {
        amount = bound(amount, 1, 1_000_000e6);
        bps = uint16(bound(bps, 0, 500));
        _fee(bps);
        vm.prank(tipper);
        tips.tipToken(creator, amount, REF);
        uint256 fee = amount * bps / 10_000;
        assertEq(usdc.balanceOf(treasury), fee);
        assertEq(usdc.balanceOf(creator), amount - fee);
        assertEq(usdc.balanceOf(address(tips)), 0);
    }

    function testFuzz_quote_adds_up_and_respects_the_cap(uint256 amount, uint16 bps) public {
        amount = bound(amount, 0, type(uint256).max / 500);
        bps = uint16(bound(bps, 0, 500));
        _fee(bps);
        (uint256 toCreator, uint256 fee) = tips.quote(amount);
        assertEq(toCreator + fee, amount);
        assertLe(fee * 10_000, amount * 500);
    }

    function testFuzz_fee_above_the_cap_is_refused(uint16 bps) public {
        bps = uint16(bound(bps, 501, type(uint16).max));
        vm.prank(owner);
        vm.expectRevert(BerylFees.FeeTooHigh.selector);
        tips.setFee(bps);
    }
}
