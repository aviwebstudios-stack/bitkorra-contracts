// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {BerylFees} from "./BerylFees.sol";

/// @title BerylPaidPosts
/// @notice Paid posts: a creator registers a post with a price in ETH or the accepted token
///         (Sepolia: Circle's test USDC). A buyer pays the creator directly and the contract
///         records their access (`hasAccess[post][buyer]`). The creator can change the price
///         or close and reopen sales; access, once bought, is never taken away.
/// @dev A post's key is keccak256(creator, postId), so nobody can register someone else's
///      post. `postId` is a hash of Beryl's post id, never the content. Checks, then effects
///      (access recorded), then the transfers, under nonReentrant. The contract never holds
///      money and has no receive function. Test networks only: docs/contracts.md.
contract BerylPaidPosts is BerylFees, ReentrancyGuard {
    using SafeERC20 for IERC20;

    enum Currency {
        ETH,
        TOKEN
    }

    struct Post {
        address creator;
        bool open;
        Currency currency;
        uint64 sales;
        uint256 price;
    }

    IERC20 public immutable token;

    mapping(bytes32 key => Post) public posts;
    mapping(bytes32 key => mapping(address buyer => bool)) public hasAccess;

    event PostRegistered(
        bytes32 indexed key, address indexed creator, bytes32 postId, Currency currency, uint256 price
    );
    event PriceChanged(bytes32 indexed key, uint256 oldPrice, uint256 newPrice);
    event SalesStatusChanged(bytes32 indexed key, bool open);
    /// @param token address(0) for ETH.
    event PostPurchased(
        bytes32 indexed key, address indexed creator, address indexed buyer, address token, uint256 price, uint256 fee
    );

    error AlreadyRegistered();
    error UnknownPost();
    error NotCreator();
    error ZeroPrice();
    error SalesClosed();
    error WrongCurrency();
    error SelfPurchase();
    error AlreadyBought();
    error WrongPayment();
    error PriceAboveMax();

    constructor(address owner_, address treasury_, IERC20 token_) BerylFees(owner_, treasury_) {
        if (address(token_) == address(0)) revert ZeroAddress();
        token = token_;
    }

    function postKey(address creator, bytes32 postId) public pure returns (bytes32) {
        return keccak256(abi.encode(creator, postId));
    }

    function registerPost(bytes32 postId, uint256 price, Currency currency) external returns (bytes32 key) {
        if (price == 0) revert ZeroPrice();
        key = postKey(msg.sender, postId);
        if (posts[key].creator != address(0)) revert AlreadyRegistered();
        posts[key] = Post({creator: msg.sender, open: true, currency: currency, sales: 0, price: price});
        emit PostRegistered(key, msg.sender, postId, currency, price);
    }

    function updatePrice(bytes32 key, uint256 newPrice) external {
        Post storage p = _own(key);
        if (newPrice == 0) revert ZeroPrice();
        emit PriceChanged(key, p.price, newPrice);
        p.price = newPrice;
    }

    /// @notice Closes (false) or reopens (true) sales. People who bought keep access.
    function setSalesOpen(bytes32 key, bool open) external {
        Post storage p = _own(key);
        p.open = open;
        emit SalesStatusChanged(key, open);
    }

    /// @notice Pays exactly the price in ETH: anything else (for example a price changed
    ///         since the buyer looked) reverts, so nothing is ever overpaid or left behind.
    function buyWithEth(bytes32 key) external payable nonReentrant {
        Post storage p = _sellable(key, Currency.ETH);
        uint256 price = p.price;
        if (msg.value != price) revert WrongPayment();
        (uint256 toCreator, uint256 fee) = _record(key, p);
        emit PostPurchased(key, p.creator, msg.sender, address(0), price, fee);
        _sendEth(p.creator, toCreator);
        _sendEth(treasury, fee);
    }

    /// @notice Pays the price in the token. `maxPrice` is the price the buyer saw: a higher
    ///         price now reverts instead of charging more. Needs an allowance of the price.
    function buyWithToken(bytes32 key, uint256 maxPrice) external nonReentrant {
        Post storage p = _sellable(key, Currency.TOKEN);
        uint256 price = p.price;
        if (price > maxPrice) revert PriceAboveMax();
        (uint256 toCreator, uint256 fee) = _record(key, p);
        emit PostPurchased(key, p.creator, msg.sender, address(token), price, fee);
        token.safeTransferFrom(msg.sender, p.creator, toCreator);
        if (fee > 0) token.safeTransferFrom(msg.sender, treasury, fee);
    }

    function _own(bytes32 key) private view returns (Post storage p) {
        p = posts[key];
        if (p.creator == address(0)) revert UnknownPost();
        if (p.creator != msg.sender) revert NotCreator();
    }

    function _sellable(bytes32 key, Currency currency) private view returns (Post storage p) {
        p = posts[key];
        if (p.creator == address(0)) revert UnknownPost();
        if (!p.open) revert SalesClosed();
        if (p.currency != currency) revert WrongCurrency();
        if (p.creator == msg.sender) revert SelfPurchase();
        if (hasAccess[key][msg.sender]) revert AlreadyBought();
    }

    /// @dev Effects before any transfer: access and the sale count.
    function _record(bytes32 key, Post storage p) private returns (uint256 toCreator, uint256 fee) {
        hasAccess[key][msg.sender] = true;
        p.sales += 1;
        return quote(p.price);
    }
}
