// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {BerylTips} from "../../src/BerylTips.sol";
import {BerylPaidPosts} from "../../src/BerylPaidPosts.sol";
import {MockERC20} from "../mocks/Mocks.sol";

/// @notice Phase W-2: random tips, purchases (full and short payments), price changes,
///         sales closed and reopened and fee changes, by several people. Ghost variables
///         record what each creator and the treasury must have received and who paid.
contract CreatorHandler is Test {
    BerylTips public tips;
    BerylPaidPosts public posts;
    MockERC20 public usdc;
    address public owner;
    address public treasury;
    address[] public actors;
    bytes32[] public keys;

    mapping(address => uint256) public expectedEth;
    mapping(address => uint256) public expectedToken;
    mapping(address => uint256) public spentEth;
    mapping(address => uint256) public spentToken;
    uint256 public expectedTreasuryEth;
    uint256 public expectedTreasuryToken;
    mapping(bytes32 => mapping(address => bool)) public paid;
    bool public accessWithoutPayment;
    uint256 public tipsMade;
    uint256 public purchases;
    uint256 public refused;

    constructor(BerylTips tips_, BerylPaidPosts posts_, MockERC20 usdc_, address owner_, address treasury_) {
        (tips, posts, usdc, owner, treasury) = (tips_, posts_, usdc_, owner_, treasury_);
        for (uint256 i; i < 4; i++) {
            address a = makeAddr(string(abi.encodePacked("actor", vm.toString(i))));
            actors.push(a);
            vm.deal(a, 10_000 ether);
            usdc.mint(a, 10_000_000e6);
            vm.startPrank(a);
            usdc.approve(address(tips), type(uint256).max);
            usdc.approve(address(posts), type(uint256).max);
            vm.stopPrank();
        }
        // Each actor sells one post in ETH and one in the token.
        for (uint256 i; i < 4; i++) {
            vm.startPrank(actors[i]);
            keys.push(posts.registerPost(bytes32(i), 0.01 ether, BerylPaidPosts.Currency.ETH));
            keys.push(posts.registerPost(bytes32(i + 100), 5e6, BerylPaidPosts.Currency.TOKEN));
            vm.stopPrank();
        }
    }

    function actorCount() external view returns (uint256) {
        return actors.length;
    }

    function keyCount() external view returns (uint256) {
        return keys.length;
    }

    function _split(uint256 amount, uint16 bps) private pure returns (uint256 toCreator, uint256 fee) {
        fee = amount * bps / 10_000;
        toCreator = amount - fee;
    }

    function tipEth(uint256 from, uint256 to, uint256 amount) external {
        address a = actors[from % 4];
        address c = actors[to % 4];
        if (a == c) return;
        amount = bound(amount, 1, 1 ether);
        (uint256 toCreator, uint256 fee) = _split(amount, tips.feeBps());
        vm.prank(a);
        tips.tipEth{value: amount}(c, keccak256("ref"));
        spentEth[a] += amount;
        expectedEth[c] += toCreator;
        expectedTreasuryEth += fee;
        tipsMade++;
    }

    function tipToken(uint256 from, uint256 to, uint256 amount) external {
        address a = actors[from % 4];
        address c = actors[to % 4];
        if (a == c) return;
        amount = bound(amount, 1, 1_000e6);
        (uint256 toCreator, uint256 fee) = _split(amount, tips.feeBps());
        vm.prank(a);
        tips.tipToken(c, amount, keccak256("ref"));
        spentToken[a] += amount;
        expectedToken[c] += toCreator;
        expectedTreasuryToken += fee;
        tipsMade++;
    }

    /// Pays the right amount, or (when `short`) one unit less, which must never give access.
    function buy(uint256 from, uint256 k, bool short) external {
        address a = actors[from % 4];
        bytes32 key = keys[k % keys.length];
        (address creator, bool open, BerylPaidPosts.Currency currency,, uint256 price) = posts.posts(key);
        bool ok;
        if (currency == BerylPaidPosts.Currency.ETH) {
            uint256 value = short ? price - 1 : price;
            vm.prank(a);
            (ok,) = address(posts).call{value: value}(abi.encodeCall(BerylPaidPosts.buyWithEth, (key)));
        } else {
            uint256 maxPrice = short ? price - 1 : price;
            vm.prank(a);
            (ok,) = address(posts).call(abi.encodeCall(BerylPaidPosts.buyWithToken, (key, maxPrice)));
        }
        bool shouldWork = !short && open && a != creator && !paid[key][a];
        if (ok != shouldWork) accessWithoutPayment = true;
        if (!ok) {
            refused++;
            return;
        }
        (uint256 toCreator, uint256 fee) = _split(price, posts.feeBps());
        if (currency == BerylPaidPosts.Currency.ETH) {
            spentEth[a] += price;
            expectedEth[creator] += toCreator;
            expectedTreasuryEth += fee;
        } else {
            spentToken[a] += price;
            expectedToken[creator] += toCreator;
            expectedTreasuryToken += fee;
        }
        paid[key][a] = true;
        purchases++;
    }

    function setPrice(uint256 k, uint256 price) external {
        bytes32 key = keys[k % keys.length];
        (address creator,, BerylPaidPosts.Currency currency,,) = posts.posts(key);
        price = currency == BerylPaidPosts.Currency.ETH ? bound(price, 2, 1 ether) : bound(price, 2, 1_000e6);
        vm.prank(creator);
        posts.updatePrice(key, price);
    }

    function setOpen(uint256 k, bool open) external {
        bytes32 key = keys[k % keys.length];
        (address creator,,,,) = posts.posts(key);
        vm.prank(creator);
        posts.setSalesOpen(key, open);
    }

    function setFee(uint16 bps) external {
        bps = uint16(bound(bps, 0, 500));
        vm.startPrank(owner);
        tips.setFee(bps);
        posts.setFee(bps);
        vm.stopPrank();
    }
}

contract BerylCreatorInvariantTest is Test {
    CreatorHandler handler;
    BerylTips tips;
    BerylPaidPosts posts;
    MockERC20 usdc;
    address owner = makeAddr("owner");
    address treasury = makeAddr("treasury");

    function setUp() public {
        usdc = new MockERC20("USD Coin", "USDC", 6);
        tips = new BerylTips(owner, treasury, usdc);
        posts = new BerylPaidPosts(owner, treasury, usdc);
        handler = new CreatorHandler(tips, posts, usdc, owner, treasury);
        targetContract(address(handler));
    }

    /// No ETH or token is ever left in either contract.
    function invariant_contracts_hold_nothing() public view {
        assertEq(address(tips).balance, 0);
        assertEq(address(posts).balance, 0);
        assertEq(usdc.balanceOf(address(tips)), 0);
        assertEq(usdc.balanceOf(address(posts)), 0);
    }

    /// The treasury holds exactly the fees.
    function invariant_treasury_gets_exactly_the_fees() public view {
        assertEq(treasury.balance, handler.expectedTreasuryEth());
        assertEq(usdc.balanceOf(treasury), handler.expectedTreasuryToken());
    }

    /// Every creator received exactly price (or tip) minus the fee for each payment to them,
    /// and every payer lost exactly what they paid: each actor's balance is
    /// start - spent + received, to the unit.
    function invariant_creators_receive_price_minus_fee() public view {
        uint256 n = handler.actorCount();
        for (uint256 i; i < n; i++) {
            address a = handler.actors(i);
            assertEq(a.balance, 10_000 ether - handler.spentEth(a) + handler.expectedEth(a));
            assertEq(usdc.balanceOf(a), 10_000_000e6 - handler.spentToken(a) + handler.expectedToken(a));
        }
    }

    /// Access exists exactly for those who paid in full.
    function invariant_access_only_after_full_payment() public view {
        assertFalse(handler.accessWithoutPayment());
        uint256 n = handler.actorCount();
        uint256 k = handler.keyCount();
        for (uint256 i; i < k; i++) {
            bytes32 key = handler.keys(i);
            for (uint256 j; j < n; j++) {
                address a = handler.actors(j);
                assertEq(posts.hasAccess(key, a), handler.paid(key, a));
            }
        }
    }
}
