// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Test} from "forge-std/Test.sol";
import {BerylEscrow} from "../../src/BerylEscrow.sol";
import {BlacklistToken} from "../mocks/Mocks.sol";

/// @notice Phase I-8: random escrows in ETH and the token by four people, marked paid, released,
///         refunded after time passes, cancelled, disputed and resolved, with fee changes and
///         blacklisting. Ghosts record what every escrow should pay and to whom.
contract EscrowHandler is Test {
    BerylEscrow public esc;
    BlacklistToken public usdc;
    address public owner;
    address public arbiter;
    address public treasury;
    address[] public actors;
    bytes32[] public keys;
    /// Every address that could ever receive money: the four actors and the treasury.
    mapping(address => bool) public allowed;
    mapping(bytes32 => bool) public settled;
    uint256 public settlements;
    bool public doubleSettle;
    uint256 public nonce;

    constructor(BerylEscrow esc_, BlacklistToken usdc_, address owner_, address arbiter_, address treasury_) {
        (esc, usdc, owner, arbiter, treasury) = (esc_, usdc_, owner_, arbiter_, treasury_);
        allowed[treasury] = true;
        for (uint256 i; i < 4; i++) {
            address a = makeAddr(string(abi.encodePacked("actor", vm.toString(i))));
            actors.push(a);
            allowed[a] = true;
            vm.deal(a, 1_000 ether);
            usdc.mint(a, 1_000_000e6);
            vm.prank(a);
            usdc.approve(address(esc), type(uint256).max);
        }
    }

    function keyCount() external view returns (uint256) {
        return keys.length;
    }

    function actorCount() external view returns (uint256) {
        return actors.length;
    }

    function fund(uint256 s, uint256 b, uint256 amount, bool eth) external {
        address seller = actors[s % 4];
        address buyer = actors[b % 4];
        if (seller == buyer) return;
        nonce++;
        bytes32 orderId = keccak256(abi.encode(nonce));
        uint64 deadline = uint64(block.timestamp + 30 minutes);
        vm.prank(seller);
        bytes32 key = eth
            ? esc.fundEth{value: bound(amount, 1, 10 ether)}(orderId, buyer, deadline)
            : esc.fundToken(orderId, buyer, bound(amount, 1, 10_000e6), deadline);
        keys.push(key);
    }

    function _escrow(uint256 k) private view returns (bytes32 key, address seller, address buyer) {
        key = keys[k % keys.length];
        (seller, buyer,,,,,) = esc.escrows(key);
    }

    function _act(address who, bytes memory call, bytes32 key) private {
        vm.prank(who);
        (bool ok,) = address(esc).call(call);
        if (ok && _isSettlement(call)) {
            if (settled[key]) doubleSettle = true;
            settled[key] = true;
            settlements++;
        }
    }

    function _isSettlement(bytes memory call) private pure returns (bool) {
        bytes4 sel = bytes4(call);
        return sel == BerylEscrow.release.selector || sel == BerylEscrow.refund.selector || sel == BerylEscrow.cancel.selector || sel == BerylEscrow.resolve.selector;
    }

    function markPaid(uint256 k) external {
        if (keys.length == 0) return;
        (bytes32 key,, address buyer) = _escrow(k);
        _act(buyer, abi.encodeCall(BerylEscrow.markPaid, (key)), key);
    }

    function dispute(uint256 k, bool bySeller) external {
        if (keys.length == 0) return;
        (bytes32 key, address seller, address buyer) = _escrow(k);
        _act(bySeller ? seller : buyer, abi.encodeCall(BerylEscrow.openDispute, (key)), key);
    }

    function release(uint256 k) external {
        if (keys.length == 0) return;
        (bytes32 key, address seller,) = _escrow(k);
        _act(seller, abi.encodeCall(BerylEscrow.release, (key)), key);
    }

    function refund(uint256 k) external {
        if (keys.length == 0) return;
        (bytes32 key, address seller,) = _escrow(k);
        _act(seller, abi.encodeCall(BerylEscrow.refund, (key)), key);
    }

    function cancel(uint256 k) external {
        if (keys.length == 0) return;
        (bytes32 key,, address buyer) = _escrow(k);
        _act(buyer, abi.encodeCall(BerylEscrow.cancel, (key)), key);
    }

    function resolve(uint256 k, bool toBuyer) external {
        if (keys.length == 0) return;
        (bytes32 key,,) = _escrow(k);
        _act(arbiter, abi.encodeCall(BerylEscrow.resolve, (key, toBuyer)), key);
    }

    /// Someone who isn't a party tries every ending: it must never work.
    function attack(uint256 k, uint256 which) external {
        if (keys.length == 0) return;
        (bytes32 key,,) = _escrow(k);
        address thief = vm.addr(uint256(keccak256("thief")));
        bytes memory call = which % 4 == 0
            ? abi.encodeCall(BerylEscrow.release, (key))
            : which % 4 == 1 ? abi.encodeCall(BerylEscrow.refund, (key)) : which % 4 == 2 ? abi.encodeCall(BerylEscrow.cancel, (key)) : abi.encodeCall(BerylEscrow.resolve, (key, true));
        vm.prank(thief);
        (bool ok,) = address(esc).call(call);
        if (ok) doubleSettle = true;
    }

    function wait(uint32 secs) external {
        vm.warp(block.timestamp + bound(secs, 1, 2 hours));
    }

    function setFee(uint16 bps) external {
        vm.prank(owner);
        esc.setFee(uint16(bound(bps, 0, 200)));
    }

    function blacklist(uint256 a, bool on) external {
        usdc.setBlacklisted(actors[a % 4], on);
    }

    function withdraw(uint256 a, bool eth) external {
        vm.prank(actors[a % 4]);
        (bool ok,) = address(esc).call(abi.encodeCall(BerylEscrow.withdraw, (eth ? address(0) : address(usdc))));
        ok;
    }
}

contract BerylEscrowInvariantTest is Test {
    EscrowHandler handler;
    BerylEscrow esc;
    BlacklistToken usdc;
    address owner = makeAddr("owner");
    address arbiter = makeAddr("arbiter");
    address treasury = makeAddr("treasury");

    function setUp() public {
        vm.warp(1_800_000_000);
        usdc = new BlacklistToken();
        esc = new BerylEscrow(owner, arbiter, treasury, usdc);
        handler = new EscrowHandler(esc, usdc, owner, arbiter, treasury);
        targetContract(address(handler));
    }

    /// What the contract holds is exactly the sum of open escrows plus payouts waiting to be withdrawn.
    function invariant_balance_equals_open_escrows_plus_owed() public view {
        uint256 openEth;
        uint256 openToken;
        uint256 n = handler.keyCount();
        for (uint256 i; i < n; i++) {
            (,, address token,,, BerylEscrow.State s, uint256 amount) = esc.escrows(handler.keys(i));
            if (s == BerylEscrow.State.Funded || s == BerylEscrow.State.Paid || s == BerylEscrow.State.Disputed) {
                if (token == address(0)) openEth += amount;
                else openToken += amount;
            }
        }
        assertEq(esc.locked(address(0)), openEth);
        assertEq(esc.locked(address(usdc)), openToken);
        assertEq(address(esc).balance, openEth + esc.totalOwed(address(0)));
        assertEq(usdc.balanceOf(address(esc)), openToken + esc.totalOwed(address(usdc)));
    }

    /// Money only ever reaches the four parties and the treasury: the total among them never changes.
    function invariant_funds_only_go_to_seller_buyer_or_treasury() public view {
        uint256 eth = treasury.balance + address(esc).balance;
        uint256 tok = usdc.balanceOf(treasury) + usdc.balanceOf(address(esc));
        uint256 n = handler.actorCount();
        for (uint256 i; i < n; i++) {
            eth += handler.actors(i).balance;
            tok += usdc.balanceOf(handler.actors(i));
        }
        assertEq(eth, n * 1_000 ether);
        assertEq(tok, n * 1_000_000e6);
        address thief = vm.addr(uint256(keccak256("thief")));
        assertEq(thief.balance, 0);
        assertEq(usdc.balanceOf(thief), 0);
    }

    /// No escrow ever settles twice, and no outsider ever settles one.
    function invariant_no_double_release_or_refund() public view {
        assertFalse(handler.doubleSettle());
    }
}
