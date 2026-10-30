// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Ownable, Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title BerylEscrow
/// @notice The P2P marketplace's escrow on Sepolia (phase I-8), the same design as the Solana
///         program (solana/programs/beryl-escrow). A seller locks ETH or the accepted token
///         (Circle's test USDC) for one order:
///         - the seller can release to the buyer at any time;
///         - the buyer marks it paid before the deadline, which stops the seller reclaiming;
///         - after the deadline, if never marked paid, the seller reclaims it;
///         - the buyer can cancel before paying (back to the seller);
///         - after "paid", either side can open a dispute; then only the arbiter resolves it
///           (to the buyer or back to the seller), or the seller can still release.
///         Money only goes to the escrow's seller, its buyer, or (a release's fee) the treasury.
///         The fee is fixed when the escrow is funded: 0 by default, never above 2%.
/// @dev Checks, then effects (the state and the locked totals), then the transfers, under
///      nonReentrant. A payout that can't be delivered (a contract that refuses ETH, a
///      blacklisted USDC address) is credited to `owed` for that address to withdraw, so no
///      escrow can be frozen by its counterparty. The arbiter changes in two steps, proposed by
///      the owner (Ownable2Step). Test networks only: mainnet needs a multisig owner and arbiter
///      and an audit (docs/contracts.md).
contract BerylEscrow is Ownable2Step, ReentrancyGuard {
    using SafeERC20 for IERC20;

    uint16 public constant MAX_FEE_BPS = 200;
    uint256 internal constant BPS = 10_000;
    uint256 public constant MAX_WINDOW = 1 days;

    enum State {
        None,
        Funded,
        Paid,
        Disputed,
        Released,
        Refunded,
        Cancelled,
        ResolvedBuyer,
        ResolvedSeller
    }

    struct Escrow {
        address seller;
        address buyer;
        /// address(0) for ETH.
        address token;
        uint64 deadline;
        uint16 feeBps;
        State state;
        uint256 amount;
    }

    IERC20 public immutable token;
    uint16 public feeBps;
    address public treasury;
    address public arbiter;
    address public pendingArbiter;

    mapping(bytes32 key => Escrow) public escrows;
    /// What open escrows hold, per asset (address(0) = ETH).
    mapping(address asset => uint256) public locked;
    /// Payouts that couldn't be delivered, per asset and recipient, to withdraw.
    mapping(address asset => mapping(address who => uint256)) public owed;
    mapping(address asset => uint256) public totalOwed;

    event EscrowFunded(bytes32 indexed key, bytes32 orderId, address indexed seller, address indexed buyer, address token, uint256 amount, uint16 feeBps, uint64 deadline);
    event MarkedPaid(bytes32 indexed key);
    event DisputeOpened(bytes32 indexed key, address indexed by);
    /// @param outcome State.Released, Refunded, Cancelled, ResolvedBuyer or ResolvedSeller.
    event EscrowSettled(bytes32 indexed key, State outcome, address indexed by, uint256 toBuyer, uint256 toSeller, uint256 fee);
    event PaymentDeferred(address indexed to, address token, uint256 amount);
    event Withdrawn(address indexed to, address token, uint256 amount);
    event FeeSet(uint16 oldBps, uint16 newBps);
    event TreasurySet(address indexed oldTreasury, address indexed newTreasury);
    event ArbiterProposed(address indexed pending);
    event ArbiterAccepted(address indexed oldArbiter, address indexed newArbiter);

    error ZeroAddress();
    error FeeTooHigh();
    error ZeroAmount();
    error BadBuyer();
    error BadDeadline();
    error AlreadyExists();
    error WrongState();
    error NotSeller();
    error NotBuyer();
    error NotParty();
    error NotArbiter();
    error NotPendingArbiter();
    error DeadlinePassed();
    error DeadlineNotPassed();
    error NothingOwed();
    error EthTransferFailed();

    constructor(address owner_, address arbiter_, address treasury_, IERC20 token_) Ownable(owner_) {
        if (arbiter_ == address(0) || treasury_ == address(0) || address(token_) == address(0)) revert ZeroAddress();
        arbiter = arbiter_;
        treasury = treasury_;
        token = token_;
        emit ArbiterAccepted(address(0), arbiter_);
        emit TreasurySet(address(0), treasury_);
    }

    function keyOf(address seller, bytes32 orderId) public pure returns (bytes32) {
        return keccak256(abi.encode(seller, orderId));
    }

    // -- owner ----------------------------------------------------------------

    function setFee(uint16 newBps) external onlyOwner {
        if (newBps > MAX_FEE_BPS) revert FeeTooHigh();
        emit FeeSet(feeBps, newBps);
        feeBps = newBps;
    }

    function setTreasury(address newTreasury) external onlyOwner {
        if (newTreasury == address(0)) revert ZeroAddress();
        emit TreasurySet(treasury, newTreasury);
        treasury = newTreasury;
    }

    /// @notice Step 1 of a new arbiter. address(0) cancels.
    function proposeArbiter(address next) external onlyOwner {
        pendingArbiter = next;
        emit ArbiterProposed(next);
    }

    /// @notice Step 2: the proposed arbiter takes over.
    function acceptArbiter() external {
        if (msg.sender != pendingArbiter || msg.sender == address(0)) revert NotPendingArbiter();
        emit ArbiterAccepted(arbiter, msg.sender);
        arbiter = msg.sender;
        pendingArbiter = address(0);
    }

    // -- funding --------------------------------------------------------------

    function fundEth(bytes32 orderId, address buyer, uint64 deadline) external payable nonReentrant returns (bytes32 key) {
        key = _open(orderId, buyer, address(0), msg.value, deadline);
    }

    /// @notice Needs an allowance of `amount` for this contract first.
    function fundToken(bytes32 orderId, address buyer, uint256 amount, uint64 deadline) external nonReentrant returns (bytes32 key) {
        key = _open(orderId, buyer, address(token), amount, deadline);
        token.safeTransferFrom(msg.sender, address(this), amount);
    }

    function _open(bytes32 orderId, address buyer, address asset, uint256 amount, uint64 deadline) private returns (bytes32 key) {
        if (amount == 0) revert ZeroAmount();
        if (buyer == address(0) || buyer == msg.sender) revert BadBuyer();
        if (deadline <= block.timestamp || deadline > block.timestamp + MAX_WINDOW) revert BadDeadline();
        key = keyOf(msg.sender, orderId);
        if (escrows[key].state != State.None) revert AlreadyExists();
        escrows[key] = Escrow({seller: msg.sender, buyer: buyer, token: asset, deadline: deadline, feeBps: feeBps, state: State.Funded, amount: amount});
        locked[asset] += amount;
        emit EscrowFunded(key, orderId, msg.sender, buyer, asset, amount, feeBps, deadline);
    }

    // -- the buyer and the seller ---------------------------------------------

    function markPaid(bytes32 key) external {
        Escrow storage e = escrows[key];
        if (msg.sender != e.buyer) revert NotBuyer();
        if (e.state != State.Funded) revert WrongState();
        if (block.timestamp > e.deadline) revert DeadlinePassed();
        e.state = State.Paid;
        emit MarkedPaid(key);
    }

    function openDispute(bytes32 key) external {
        Escrow storage e = escrows[key];
        if (msg.sender != e.buyer && msg.sender != e.seller) revert NotParty();
        if (e.state != State.Paid) revert WrongState();
        e.state = State.Disputed;
        emit DisputeOpened(key, msg.sender);
    }

    /// @notice The seller releases to the buyer, at any time (also during a dispute).
    function release(bytes32 key) external nonReentrant {
        Escrow storage e = escrows[key];
        if (msg.sender != e.seller) revert NotSeller();
        if (e.state != State.Funded && e.state != State.Paid && e.state != State.Disputed) revert WrongState();
        _settle(key, e, true, State.Released);
    }

    /// @notice After the deadline, if the buyer never marked it paid.
    function refund(bytes32 key) external nonReentrant {
        Escrow storage e = escrows[key];
        if (msg.sender != e.seller) revert NotSeller();
        if (e.state != State.Funded) revert WrongState();
        if (block.timestamp <= e.deadline) revert DeadlineNotPassed();
        _settle(key, e, false, State.Refunded);
    }

    /// @notice The buyer cancels before paying: back to the seller at once.
    function cancel(bytes32 key) external nonReentrant {
        Escrow storage e = escrows[key];
        if (msg.sender != e.buyer) revert NotBuyer();
        if (e.state != State.Funded) revert WrongState();
        _settle(key, e, false, State.Cancelled);
    }

    function resolve(bytes32 key, bool toBuyer) external nonReentrant {
        if (msg.sender != arbiter) revert NotArbiter();
        Escrow storage e = escrows[key];
        if (e.state != State.Disputed) revert WrongState();
        _settle(key, e, toBuyer, toBuyer ? State.ResolvedBuyer : State.ResolvedSeller);
    }

    /// @notice Takes out a payout that couldn't be delivered.
    function withdraw(address asset) external nonReentrant {
        uint256 amount = owed[asset][msg.sender];
        if (amount == 0) revert NothingOwed();
        owed[asset][msg.sender] = 0;
        totalOwed[asset] -= amount;
        emit Withdrawn(msg.sender, asset, amount);
        if (asset == address(0)) {
            (bool ok,) = msg.sender.call{value: amount}("");
            if (!ok) revert EthTransferFailed();
        } else {
            IERC20(asset).safeTransfer(msg.sender, amount);
        }
    }

    function _settle(bytes32 key, Escrow storage e, bool toBuyer, State outcome) private {
        uint256 amount = e.amount;
        uint256 fee = toBuyer ? amount * e.feeBps / BPS : 0;
        uint256 toBuyerAmount = toBuyer ? amount - fee : 0;
        uint256 toSellerAmount = toBuyer ? 0 : amount;
        address asset = e.token;
        // Effects first: this escrow can never settle again.
        e.state = outcome;
        locked[asset] -= amount;
        emit EscrowSettled(key, outcome, msg.sender, toBuyerAmount, toSellerAmount, fee);
        _pay(asset, e.buyer, toBuyerAmount);
        _pay(asset, treasury, fee);
        _pay(asset, e.seller, toSellerAmount);
    }

    /// @dev Delivers, or credits `owed` when the recipient can't take it, so a payout never blocks a settlement.
    function _pay(address asset, address to, uint256 amount) private {
        if (amount == 0) return;
        bool ok;
        if (asset == address(0)) {
            (ok,) = to.call{value: amount}("");
        } else {
            ok = IERC20(asset).trySafeTransfer(to, amount);
        }
        if (!ok) {
            owed[asset][to] += amount;
            totalOwed[asset] += amount;
            emit PaymentDeferred(to, asset, amount);
        }
    }
}
