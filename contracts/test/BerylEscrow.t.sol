// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {BerylEscrow} from "../src/BerylEscrow.sol";
import {BlacklistToken, RejectingCreator} from "./mocks/Mocks.sol";

/// @notice Phase I-8: every path of BerylEscrow in ETH and the token, the fee, the arbiter and the refusals.
contract BerylEscrowTest is Test {
    BerylEscrow esc;
    BlacklistToken usdc;
    address owner = makeAddr("owner");
    address arbiter = makeAddr("arbiter");
    address treasury = makeAddr("treasury");
    address seller = makeAddr("seller");
    address buyer = makeAddr("buyer");
    address stranger = makeAddr("stranger");
    bytes32 constant ORDER = keccak256("beryl:p2p:order:1");
    uint64 constant WINDOW = 30 minutes;

    event EscrowSettled(bytes32 indexed key, BerylEscrow.State outcome, address indexed by, uint256 toBuyer, uint256 toSeller, uint256 fee);

    function setUp() public {
        vm.warp(1_800_000_000);
        usdc = new BlacklistToken();
        esc = new BerylEscrow(owner, arbiter, treasury, usdc);
        vm.deal(seller, 100 ether);
        usdc.mint(seller, 1_000e6);
        vm.prank(seller);
        usdc.approve(address(esc), type(uint256).max);
    }

    function _fundEth(uint256 amount) internal returns (bytes32 key) {
        vm.prank(seller);
        key = esc.fundEth{value: amount}(ORDER, buyer, uint64(block.timestamp) + WINDOW);
    }

    function _fundToken(uint256 amount) internal returns (bytes32 key) {
        vm.prank(seller);
        key = esc.fundToken(ORDER, buyer, amount, uint64(block.timestamp) + WINDOW);
    }

    function _state(bytes32 key) internal view returns (BerylEscrow.State s) {
        (,,,,, s,) = esc.escrows(key);
    }

    function _dispute(bytes32 key) internal {
        vm.prank(buyer);
        esc.markPaid(key);
        vm.prank(buyer);
        esc.openDispute(key);
    }

    // -- setup -----------------------------------------------------------------

    function test_deploys_with_no_fee_and_refuses_zero_addresses() public {
        assertEq(esc.feeBps(), 0);
        assertEq(esc.arbiter(), arbiter);
        vm.expectRevert(BerylEscrow.ZeroAddress.selector);
        new BerylEscrow(owner, address(0), treasury, usdc);
        vm.expectRevert(BerylEscrow.ZeroAddress.selector);
        new BerylEscrow(owner, arbiter, address(0), usdc);
    }

    function test_fee_is_capped_at_2_percent_and_owner_only() public {
        vm.prank(stranger);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, stranger));
        esc.setFee(1);
        vm.startPrank(owner);
        vm.expectRevert(BerylEscrow.FeeTooHigh.selector);
        esc.setFee(201);
        esc.setFee(200);
        vm.expectRevert(BerylEscrow.ZeroAddress.selector);
        esc.setTreasury(address(0));
        vm.stopPrank();
    }

    function test_arbiter_changes_in_two_steps_proposed_by_the_owner() public {
        address next = makeAddr("next");
        vm.prank(arbiter);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, arbiter));
        esc.proposeArbiter(next);
        vm.prank(owner);
        esc.proposeArbiter(next);
        assertEq(esc.arbiter(), arbiter);
        vm.prank(stranger);
        vm.expectRevert(BerylEscrow.NotPendingArbiter.selector);
        esc.acceptArbiter();
        vm.prank(next);
        esc.acceptArbiter();
        assertEq(esc.arbiter(), next);
        assertEq(esc.pendingArbiter(), address(0));
        // The old arbiter can no longer resolve.
        bytes32 key = _fundEth(1 ether);
        _dispute(key);
        vm.prank(arbiter);
        vm.expectRevert(BerylEscrow.NotArbiter.selector);
        esc.resolve(key, true);
    }

    // -- funding ---------------------------------------------------------------

    function test_funding_locks_the_money_and_refuses_bad_terms() public {
        bytes32 key = _fundEth(1 ether);
        assertEq(address(esc).balance, 1 ether);
        assertEq(esc.locked(address(0)), 1 ether);
        assertEq(uint8(_state(key)), uint8(BerylEscrow.State.Funded));
        vm.startPrank(seller);
        vm.expectRevert(BerylEscrow.AlreadyExists.selector);
        esc.fundEth{value: 1}(ORDER, buyer, uint64(block.timestamp) + WINDOW);
        bytes32 o2 = keccak256("2");
        vm.expectRevert(BerylEscrow.ZeroAmount.selector);
        esc.fundEth{value: 0}(o2, buyer, uint64(block.timestamp) + WINDOW);
        vm.expectRevert(BerylEscrow.BadBuyer.selector);
        esc.fundEth{value: 1}(o2, seller, uint64(block.timestamp) + WINDOW);
        vm.expectRevert(BerylEscrow.BadBuyer.selector);
        esc.fundEth{value: 1}(o2, address(0), uint64(block.timestamp) + WINDOW);
        vm.expectRevert(BerylEscrow.BadDeadline.selector);
        esc.fundEth{value: 1}(o2, buyer, uint64(block.timestamp));
        vm.expectRevert(BerylEscrow.BadDeadline.selector);
        esc.fundEth{value: 1}(o2, buyer, uint64(block.timestamp + 1 days + 1));
        vm.stopPrank();
        // Plain ETH is refused (no receive).
        (bool ok,) = address(esc).call{value: 1}("");
        assertFalse(ok);
    }

    // -- endings ----------------------------------------------------------------

    function test_release_pays_the_buyer_and_the_fee_fixed_at_funding() public {
        vm.prank(owner);
        esc.setFee(150);
        bytes32 key = _fundToken(100e6);
        vm.prank(owner);
        esc.setFee(0);
        vm.prank(buyer);
        esc.markPaid(key);
        vm.prank(buyer);
        vm.expectRevert(BerylEscrow.NotSeller.selector);
        esc.release(key);
        vm.expectEmit(address(esc));
        emit EscrowSettled(key, BerylEscrow.State.Released, seller, 98.5e6, 0, 1.5e6);
        vm.prank(seller);
        esc.release(key);
        assertEq(usdc.balanceOf(buyer), 98.5e6);
        assertEq(usdc.balanceOf(treasury), 1.5e6);
        assertEq(usdc.balanceOf(address(esc)), 0);
        vm.prank(seller);
        vm.expectRevert(BerylEscrow.WrongState.selector);
        esc.release(key);
    }

    function test_mark_paid_only_by_the_buyer_before_the_deadline() public {
        bytes32 key = _fundEth(1 ether);
        vm.prank(seller);
        vm.expectRevert(BerylEscrow.NotBuyer.selector);
        esc.markPaid(key);
        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(buyer);
        vm.expectRevert(BerylEscrow.DeadlinePassed.selector);
        esc.markPaid(key);
    }

    function test_refund_only_after_the_deadline_and_never_after_paid() public {
        bytes32 key = _fundEth(1 ether);
        vm.prank(seller);
        vm.expectRevert(BerylEscrow.DeadlineNotPassed.selector);
        esc.refund(key);
        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(buyer);
        vm.expectRevert(BerylEscrow.NotSeller.selector);
        esc.refund(key);
        uint256 before = seller.balance;
        vm.prank(seller);
        esc.refund(key);
        assertEq(seller.balance - before, 1 ether);
        // Marked paid: the deadline no longer lets the seller take it back.
        bytes32 k2 = keccak256(abi.encode(seller, keccak256("x")));
        vm.prank(seller);
        k2 = esc.fundEth{value: 1 ether}(keccak256("x"), buyer, uint64(block.timestamp) + WINDOW);
        vm.prank(buyer);
        esc.markPaid(k2);
        vm.warp(block.timestamp + 10 days);
        vm.prank(seller);
        vm.expectRevert(BerylEscrow.WrongState.selector);
        esc.refund(k2);
    }

    function test_buyer_cancels_before_paying_only() public {
        bytes32 key = _fundEth(1 ether);
        vm.prank(seller);
        vm.expectRevert(BerylEscrow.NotBuyer.selector);
        esc.cancel(key);
        uint256 before = seller.balance;
        vm.prank(buyer);
        esc.cancel(key);
        assertEq(seller.balance - before, 1 ether);
        assertEq(uint8(_state(key)), uint8(BerylEscrow.State.Cancelled));
    }

    function test_disputes_and_the_arbiter() public {
        bytes32 key = _fundEth(1 ether);
        vm.prank(buyer);
        vm.expectRevert(BerylEscrow.WrongState.selector);
        esc.openDispute(key);
        vm.prank(buyer);
        esc.markPaid(key);
        vm.prank(stranger);
        vm.expectRevert(BerylEscrow.NotParty.selector);
        esc.openDispute(key);
        vm.prank(arbiter);
        vm.expectRevert(BerylEscrow.WrongState.selector);
        esc.resolve(key, true);
        vm.prank(seller);
        esc.openDispute(key);
        // Nobody but the arbiter resolves; the buyer can't cancel; the seller can't reclaim.
        for (uint256 i; i < 3; i++) {
            address who = i == 0 ? seller : i == 1 ? buyer : owner;
            vm.prank(who);
            vm.expectRevert(BerylEscrow.NotArbiter.selector);
            esc.resolve(key, true);
        }
        vm.prank(buyer);
        vm.expectRevert(BerylEscrow.WrongState.selector);
        esc.cancel(key);
        vm.prank(arbiter);
        esc.resolve(key, true);
        assertEq(buyer.balance, 1 ether);
        assertEq(uint8(_state(key)), uint8(BerylEscrow.State.ResolvedBuyer));
    }

    function test_resolve_to_the_seller_is_a_full_refund_without_fee() public {
        vm.prank(owner);
        esc.setFee(200);
        bytes32 key = _fundToken(50e6);
        _dispute(key);
        vm.prank(arbiter);
        esc.resolve(key, false);
        assertEq(usdc.balanceOf(seller), 1_000e6);
        assertEq(usdc.balanceOf(treasury), 0);
    }

    function test_the_seller_can_still_release_during_a_dispute() public {
        bytes32 key = _fundEth(1 ether);
        _dispute(key);
        vm.prank(seller);
        esc.release(key);
        assertEq(buyer.balance, 1 ether);
    }

    // -- recipients that can't take a payout --------------------------------------

    function test_a_buyer_that_refuses_eth_is_credited_and_withdraws_later() public {
        RejectingCreator rc = new RejectingCreator();
        vm.prank(seller);
        bytes32 key = esc.fundEth{value: 1 ether}(ORDER, address(rc), uint64(block.timestamp) + WINDOW);
        vm.prank(seller);
        esc.release(key);
        assertEq(esc.owed(address(0), address(rc)), 1 ether);
        assertEq(address(esc).balance, esc.locked(address(0)) + esc.totalOwed(address(0)));
        vm.prank(address(rc));
        vm.expectRevert(BerylEscrow.EthTransferFailed.selector);
        esc.withdraw(address(0));
        vm.prank(stranger);
        vm.expectRevert(BerylEscrow.NothingOwed.selector);
        esc.withdraw(address(0));
    }

    function test_a_blacklisted_buyer_never_freezes_the_escrow() public {
        bytes32 key = _fundToken(10e6);
        usdc.setBlacklisted(buyer, true);
        vm.prank(seller);
        esc.release(key);
        assertEq(esc.owed(address(usdc), buyer), 10e6);
        usdc.setBlacklisted(buyer, false);
        vm.prank(buyer);
        esc.withdraw(address(usdc));
        assertEq(usdc.balanceOf(buyer), 10e6);
        assertEq(usdc.balanceOf(address(esc)), 0);
    }

    // -- fuzz -----------------------------------------------------------------------

    function testFuzz_release_pays_amount_minus_fee(uint96 amount, uint16 bps) public {
        amount = uint96(bound(amount, 1, 50 ether));
        bps = uint16(bound(bps, 0, 200));
        vm.prank(owner);
        esc.setFee(bps);
        bytes32 key = _fundEth(amount);
        vm.prank(seller);
        esc.release(key);
        uint256 fee = uint256(amount) * bps / 10_000;
        assertEq(buyer.balance, amount - fee);
        assertEq(treasury.balance, fee);
        assertEq(address(esc).balance, 0);
    }

    function testFuzz_refund_returns_everything_and_only_after_the_deadline(uint96 amount, uint32 wait) public {
        amount = uint96(bound(amount, 1, 50 ether));
        bytes32 key = _fundEth(amount);
        vm.warp(block.timestamp + wait);
        vm.prank(seller);
        if (wait <= WINDOW) vm.expectRevert(BerylEscrow.DeadlineNotPassed.selector);
        esc.refund(key);
        if (wait > WINDOW) assertEq(seller.balance, 100 ether);
    }

    function testFuzz_no_one_but_the_parties_settles(address who) public {
        vm.assume(who != seller && who != buyer && who != arbiter);
        bytes32 key = _fundEth(1 ether);
        vm.startPrank(who);
        vm.expectRevert(BerylEscrow.NotSeller.selector);
        esc.release(key);
        vm.expectRevert(BerylEscrow.NotBuyer.selector);
        esc.cancel(key);
        vm.expectRevert(BerylEscrow.NotArbiter.selector);
        esc.resolve(key, true);
        vm.stopPrank();
        assertEq(address(esc).balance, 1 ether);
    }
}
