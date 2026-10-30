// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {BerylFees} from "../src/BerylFees.sol";
import {BerylPaidPosts} from "../src/BerylPaidPosts.sol";
import {MockERC20, RejectingCreator, ReentrantCreator} from "./mocks/Mocks.sol";

/// @notice Phase W-2: register, change the price, close and reopen, buy in ETH and test USDC.
contract BerylPaidPostsTest is Test {
    BerylPaidPosts posts;
    MockERC20 usdc;
    address owner = makeAddr("owner");
    address treasury = makeAddr("treasury");
    address creator = makeAddr("creator");
    address buyer = makeAddr("buyer");
    address late = makeAddr("late");
    bytes32 constant ID = keccak256("beryl:post:1");

    event PostPurchased(
        bytes32 indexed key, address indexed creator, address indexed buyer, address token, uint256 price, uint256 fee
    );

    function setUp() public {
        usdc = new MockERC20("USD Coin", "USDC", 6);
        posts = new BerylPaidPosts(owner, treasury, usdc);
        for (uint256 i; i < 2; i++) {
            address a = i == 0 ? buyer : late;
            vm.deal(a, 100 ether);
            usdc.mint(a, 1_000_000e6);
            vm.prank(a);
            usdc.approve(address(posts), type(uint256).max);
        }
    }

    function _register(uint256 price, BerylPaidPosts.Currency c) internal returns (bytes32 key) {
        vm.prank(creator);
        key = posts.registerPost(ID, price, c);
    }

    function _post(bytes32 key) internal view returns (BerylPaidPosts.Post memory p) {
        (p.creator, p.open, p.currency, p.sales, p.price) = posts.posts(key);
    }

    // -- register, price, sales --------------------------------------------

    function test_register_once_per_creator_and_id() public {
        bytes32 key = _register(0.01 ether, BerylPaidPosts.Currency.ETH);
        assertEq(key, posts.postKey(creator, ID));
        BerylPaidPosts.Post memory p = _post(key);
        assertEq(p.creator, creator);
        assertTrue(p.open);
        assertEq(p.price, 0.01 ether);
        vm.prank(creator);
        vm.expectRevert(BerylPaidPosts.AlreadyRegistered.selector);
        posts.registerPost(ID, 1, BerylPaidPosts.Currency.TOKEN);
        // Someone else's post with the same id lives at another key: no squatting.
        vm.prank(buyer);
        bytes32 other = posts.registerPost(ID, 1, BerylPaidPosts.Currency.TOKEN);
        assertTrue(other != key);
        vm.prank(creator);
        vm.expectRevert(BerylPaidPosts.ZeroPrice.selector);
        posts.registerPost(keccak256("x"), 0, BerylPaidPosts.Currency.ETH);
    }

    function test_only_the_creator_changes_the_price_or_closes_sales() public {
        bytes32 key = _register(1 ether, BerylPaidPosts.Currency.ETH);
        vm.startPrank(buyer);
        vm.expectRevert(BerylPaidPosts.NotCreator.selector);
        posts.updatePrice(key, 1);
        vm.expectRevert(BerylPaidPosts.NotCreator.selector);
        posts.setSalesOpen(key, false);
        vm.expectRevert(BerylPaidPosts.UnknownPost.selector);
        posts.updatePrice(keccak256("nope"), 1);
        vm.stopPrank();
        vm.startPrank(creator);
        vm.expectRevert(BerylPaidPosts.ZeroPrice.selector);
        posts.updatePrice(key, 0);
        posts.updatePrice(key, 2 ether);
        posts.setSalesOpen(key, false);
        vm.stopPrank();
        assertEq(_post(key).price, 2 ether);
        assertFalse(_post(key).open);
    }

    // -- buying with ETH ----------------------------------------------------

    function test_buy_with_eth_pays_the_creator_and_records_access() public {
        vm.prank(owner);
        posts.setFee(300);
        bytes32 key = _register(0.5 ether, BerylPaidPosts.Currency.ETH);
        uint256 fee = 0.5 ether * 300 / 10_000;
        vm.expectEmit(address(posts));
        emit PostPurchased(key, creator, buyer, address(0), 0.5 ether, fee);
        vm.prank(buyer);
        posts.buyWithEth{value: 0.5 ether}(key);
        assertTrue(posts.hasAccess(key, buyer));
        assertEq(creator.balance, 0.5 ether - fee);
        assertEq(treasury.balance, fee);
        assertEq(address(posts).balance, 0);
        assertEq(_post(key).sales, 1);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.AlreadyBought.selector);
        posts.buyWithEth{value: 0.5 ether}(key);
    }

    function test_eth_payment_must_be_exactly_the_price() public {
        bytes32 key = _register(1 ether, BerylPaidPosts.Currency.ETH);
        vm.startPrank(buyer);
        vm.expectRevert(BerylPaidPosts.WrongPayment.selector);
        posts.buyWithEth{value: 1 ether - 1}(key);
        vm.expectRevert(BerylPaidPosts.WrongPayment.selector);
        posts.buyWithEth{value: 1 ether + 1}(key);
        vm.stopPrank();
        // The creator raises the price after the buyer looked: the old amount reverts.
        vm.prank(creator);
        posts.updatePrice(key, 2 ether);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.WrongPayment.selector);
        posts.buyWithEth{value: 1 ether}(key);
        assertFalse(posts.hasAccess(key, buyer));
        assertEq(buyer.balance, 100 ether);
    }

    function test_closed_sales_refuse_new_buyers_and_buyers_keep_access() public {
        bytes32 key = _register(0.1 ether, BerylPaidPosts.Currency.ETH);
        vm.prank(buyer);
        posts.buyWithEth{value: 0.1 ether}(key);
        vm.prank(creator);
        posts.setSalesOpen(key, false);
        vm.prank(late);
        vm.expectRevert(BerylPaidPosts.SalesClosed.selector);
        posts.buyWithEth{value: 0.1 ether}(key);
        assertTrue(posts.hasAccess(key, buyer));
        vm.startPrank(creator);
        posts.updatePrice(key, 1 ether);
        posts.setSalesOpen(key, true);
        vm.stopPrank();
        assertTrue(posts.hasAccess(key, buyer));
        vm.prank(late);
        posts.buyWithEth{value: 1 ether}(key);
        assertTrue(posts.hasAccess(key, late));
        assertEq(_post(key).sales, 2);
    }

    function test_eth_purchase_refusals() public {
        bytes32 key = _register(0.1 ether, BerylPaidPosts.Currency.ETH);
        vm.deal(creator, 1 ether);
        vm.prank(creator);
        vm.expectRevert(BerylPaidPosts.SelfPurchase.selector);
        posts.buyWithEth{value: 0.1 ether}(key);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.UnknownPost.selector);
        posts.buyWithEth{value: 0.1 ether}(keccak256("nope"));
        vm.prank(creator);
        bytes32 tokenKey = posts.registerPost(keccak256("t"), 1e6, BerylPaidPosts.Currency.TOKEN);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.WrongCurrency.selector);
        posts.buyWithEth{value: 1e6}(tokenKey);
    }

    function test_a_creator_that_refuses_eth_makes_the_purchase_revert() public {
        RejectingCreator rc = new RejectingCreator();
        vm.prank(address(rc));
        bytes32 key = posts.registerPost(ID, 1 ether, BerylPaidPosts.Currency.ETH);
        vm.prank(buyer);
        vm.expectRevert(BerylFees.EthTransferFailed.selector);
        posts.buyWithEth{value: 1 ether}(key);
        assertFalse(posts.hasAccess(key, buyer));
        assertEq(address(posts).balance, 0);
    }

    function test_reentering_a_purchase_is_blocked() public {
        ReentrantCreator rc = new ReentrantCreator();
        vm.prank(address(rc));
        bytes32 key = posts.registerPost(ID, 1 ether, BerylPaidPosts.Currency.ETH);
        vm.prank(address(rc));
        bytes32 other = posts.registerPost(keccak256("2"), 1 ether, BerylPaidPosts.Currency.ETH);
        rc.arm(address(posts), abi.encodeCall(BerylPaidPosts.buyWithEth, (other)));
        vm.prank(buyer);
        posts.buyWithEth{value: 1 ether}(key);
        assertTrue(rc.reentered());
        assertTrue(rc.reentryBlocked());
        assertTrue(posts.hasAccess(key, buyer));
        assertEq(address(posts).balance, 0);
    }

    function test_plain_eth_sent_to_the_contract_is_refused() public {
        vm.prank(buyer);
        (bool ok,) = address(posts).call{value: 1 ether}("");
        assertFalse(ok);
    }

    // -- buying with the token ----------------------------------------------

    function test_buy_with_token_pays_the_creator_and_the_fee() public {
        vm.prank(owner);
        posts.setFee(100);
        bytes32 key = _register(3e6, BerylPaidPosts.Currency.TOKEN);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.PriceAboveMax.selector);
        posts.buyWithToken(key, 3e6 - 1);
        vm.expectEmit(address(posts));
        emit PostPurchased(key, creator, buyer, address(usdc), 3e6, 30_000);
        vm.prank(buyer);
        posts.buyWithToken(key, 3e6);
        assertTrue(posts.hasAccess(key, buyer));
        assertEq(usdc.balanceOf(creator), 3e6 - 30_000);
        assertEq(usdc.balanceOf(treasury), 30_000);
        assertEq(usdc.balanceOf(address(posts)), 0);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.AlreadyBought.selector);
        posts.buyWithToken(key, 3e6);
    }

    function test_token_purchase_with_a_lower_price_pays_the_actual_price() public {
        bytes32 key = _register(3e6, BerylPaidPosts.Currency.TOKEN);
        vm.prank(creator);
        posts.updatePrice(key, 1e6);
        vm.prank(buyer);
        posts.buyWithToken(key, 3e6);
        assertEq(usdc.balanceOf(creator), 1e6);
    }

    function test_token_purchase_refusals() public {
        bytes32 key = _register(1e6, BerylPaidPosts.Currency.TOKEN);
        vm.prank(buyer);
        bytes32 ethKey = posts.registerPost(keccak256("e"), 1 ether, BerylPaidPosts.Currency.ETH);
        vm.prank(late);
        vm.expectRevert(BerylPaidPosts.WrongCurrency.selector);
        posts.buyWithToken(ethKey, type(uint256).max);
        // Not enough tokens: nothing is recorded.
        address poor = makeAddr("poor");
        usdc.mint(poor, 1e6 - 1);
        vm.startPrank(poor);
        usdc.approve(address(posts), type(uint256).max);
        vm.expectRevert();
        posts.buyWithToken(key, 1e6);
        vm.stopPrank();
        assertFalse(posts.hasAccess(key, poor));
        assertEq(_post(key).sales, 0);
    }

    // -- fuzz -----------------------------------------------------------------

    function testFuzz_eth_purchase_pays_price_minus_fee(uint96 price, uint16 bps) public {
        price = uint96(bound(price, 1, 50 ether));
        bps = uint16(bound(bps, 0, 500));
        vm.prank(owner);
        posts.setFee(bps);
        bytes32 key = _register(price, BerylPaidPosts.Currency.ETH);
        vm.prank(buyer);
        posts.buyWithEth{value: price}(key);
        uint256 fee = uint256(price) * bps / 10_000;
        assertEq(creator.balance, price - fee);
        assertEq(treasury.balance, fee);
        assertEq(address(posts).balance, 0);
        assertTrue(posts.hasAccess(key, buyer));
    }

    function testFuzz_access_only_after_full_token_payment(uint256 price, uint256 balance, uint16 bps) public {
        price = bound(price, 1, 1_000_000e6);
        balance = bound(balance, 0, 2_000_000e6);
        bps = uint16(bound(bps, 0, 500));
        vm.prank(owner);
        posts.setFee(bps);
        bytes32 key = _register(price, BerylPaidPosts.Currency.TOKEN);
        address b = makeAddr("fuzzBuyer");
        usdc.mint(b, balance);
        vm.startPrank(b);
        usdc.approve(address(posts), type(uint256).max);
        if (balance < price) vm.expectRevert();
        posts.buyWithToken(key, price);
        vm.stopPrank();
        uint256 fee = price * bps / 10_000;
        if (balance < price) {
            assertFalse(posts.hasAccess(key, b));
            assertEq(usdc.balanceOf(creator), 0);
            assertEq(usdc.balanceOf(b), balance);
        } else {
            assertTrue(posts.hasAccess(key, b));
            assertEq(usdc.balanceOf(creator), price - fee);
            assertEq(usdc.balanceOf(treasury), fee);
            assertEq(usdc.balanceOf(b), balance - price);
        }
        assertEq(usdc.balanceOf(address(posts)), 0);
    }

    function testFuzz_wrong_eth_amount_never_buys(uint96 price, uint96 sent) public {
        price = uint96(bound(price, 1, 50 ether));
        sent = uint96(bound(sent, 0, 60 ether));
        vm.assume(sent != price);
        bytes32 key = _register(price, BerylPaidPosts.Currency.ETH);
        vm.prank(buyer);
        vm.expectRevert(BerylPaidPosts.WrongPayment.selector);
        posts.buyWithEth{value: sent}(key);
        assertFalse(posts.hasAccess(key, buyer));
    }
}
