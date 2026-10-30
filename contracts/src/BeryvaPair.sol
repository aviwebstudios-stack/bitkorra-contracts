// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";

/// @title Beryva Swap pool for one pair of tokens (constant product, x * y = k)
/// @notice Holds two tokens and swaps between them at the price set by their reserves,
///         keeping a 0.3% fee on every input for the liquidity providers. Its own ERC-20
///         token (BRV-LP) is each provider's share of the pool.
/// @dev Uniswap V2's pair, rewritten for Solidity 0.8 with OpenZeppelin's ERC20,
///      SafeERC20 and ReentrancyGuard. Deliberately left out: flash swaps (no callback,
///      so a swap never calls into an outside contract before its checks), the protocol
///      fee switch, and the TWAP price accumulators (not safe as an oracle on a thin
///      test pool; I-9 uses Chainlink). Tokens that charge a fee on transfer or change
///      balances on their own are not supported. Test networks only: docs/contracts.md.
contract BeryvaPair is ERC20, ReentrancyGuard {
    using SafeERC20 for IERC20;

    /// @notice LP tokens burned forever on the first deposit, so the share price can't be
    ///         pushed to a level where small deposits round to nothing.
    uint256 public constant MINIMUM_LIQUIDITY = 1000;
    /// @notice The fee kept on every swap input, in parts per thousand (3 = 0.3%).
    uint256 public constant FEE_PER_THOUSAND = 3;
    /// @dev OpenZeppelin's ERC20 refuses to mint to address(0), so the locked minimum goes here.
    address public constant DEAD = 0x000000000000000000000000000000000000dEaD;

    address public immutable factory;
    address public immutable token0;
    address public immutable token1;

    uint112 private reserve0;
    uint112 private reserve1;

    event Mint(address indexed sender, uint256 amount0, uint256 amount1, address indexed to);
    event Burn(address indexed sender, uint256 amount0, uint256 amount1, address indexed to);
    event Swap(address indexed sender, uint256 amount0In, uint256 amount1In, uint256 amount0Out, uint256 amount1Out, address indexed to);
    event Sync(uint112 reserve0, uint112 reserve1);

    error InsufficientLiquidityMinted();
    error InsufficientLiquidityBurned();
    error InsufficientOutputAmount();
    error InsufficientInputAmount();
    error InsufficientLiquidity();
    error InvalidTo();
    error KInvariant();
    error Overflow();
    error ZeroAddress();

    /// @dev Only deployed by BeryvaFactory, with the tokens already sorted and checked.
    constructor(address token0_, address token1_) ERC20("Beryva Swap LP", "BRV-LP") {
        if (token0_ == address(0) || token1_ == address(0)) revert ZeroAddress();
        factory = msg.sender;
        token0 = token0_;
        token1 = token1_;
    }

    function getReserves() public view returns (uint112, uint112) {
        return (reserve0, reserve1);
    }

    /// @notice Adds liquidity: the caller (the router) sends both tokens first, then calls this.
    /// @return liquidity The LP tokens minted to `to`.
    function mint(address to) external nonReentrant returns (uint256 liquidity) {
        (uint112 r0, uint112 r1) = (reserve0, reserve1);
        uint256 balance0 = IERC20(token0).balanceOf(address(this));
        uint256 balance1 = IERC20(token1).balanceOf(address(this));
        if (balance0 > type(uint112).max || balance1 > type(uint112).max) revert Overflow();
        uint256 amount0 = balance0 - r0;
        uint256 amount1 = balance1 - r1;

        uint256 supply = totalSupply();
        if (supply == 0) {
            uint256 root = Math.sqrt(amount0 * amount1);
            if (root <= MINIMUM_LIQUIDITY) revert InsufficientLiquidityMinted();
            liquidity = root - MINIMUM_LIQUIDITY;
            _mint(DEAD, MINIMUM_LIQUIDITY);
        } else {
            // The smaller of the two shares: anything sent beyond the pool's ratio is a gift to the pool.
            liquidity = Math.min((amount0 * supply) / r0, (amount1 * supply) / r1);
        }
        if (liquidity == 0) revert InsufficientLiquidityMinted();

        _update(balance0, balance1);
        _mint(to, liquidity);
        emit Mint(msg.sender, amount0, amount1, to);
    }

    /// @notice Removes liquidity: the caller sends LP tokens to this pool first, then calls this.
    ///         Amounts round down, so the pool never pays out more than the share.
    function burn(address to) external nonReentrant returns (uint256 amount0, uint256 amount1) {
        address t0 = token0;
        address t1 = token1;
        uint256 balance0 = IERC20(t0).balanceOf(address(this));
        uint256 balance1 = IERC20(t1).balanceOf(address(this));
        uint256 liquidity = balanceOf(address(this));

        uint256 supply = totalSupply();
        amount0 = (liquidity * balance0) / supply;
        amount1 = (liquidity * balance1) / supply;
        if (amount0 == 0 || amount1 == 0) revert InsufficientLiquidityBurned();

        _burn(address(this), liquidity);
        _update(balance0 - amount0, balance1 - amount1);
        emit Burn(msg.sender, amount0, amount1, to);
        IERC20(t0).safeTransfer(to, amount0);
        IERC20(t1).safeTransfer(to, amount1);
    }

    /// @notice Swaps: the caller sends the input token first, then asks for an output.
    ///         The new balances, less the 0.3% fee on what came in, must keep
    ///         reserve0 * reserve1 at least where it was; otherwise the whole call reverts.
    /// @dev Checks, then effects, then the one outside call (the transfer out). The
    ///      balances used for the check already leave out what is about to be sent.
    function swap(uint256 amount0Out, uint256 amount1Out, address to) external nonReentrant {
        if (amount0Out == 0 && amount1Out == 0) revert InsufficientOutputAmount();
        (uint112 r0, uint112 r1) = (reserve0, reserve1);
        if (amount0Out >= r0 || amount1Out >= r1) revert InsufficientLiquidity();
        if (to == token0 || to == token1 || to == address(this)) revert InvalidTo();

        uint256 balance0 = IERC20(token0).balanceOf(address(this)) - amount0Out;
        uint256 balance1 = IERC20(token1).balanceOf(address(this)) - amount1Out;
        uint256 amount0In = balance0 > r0 - amount0Out ? balance0 - (r0 - amount0Out) : 0;
        uint256 amount1In = balance1 > r1 - amount1Out ? balance1 - (r1 - amount1Out) : 0;
        if (amount0In == 0 && amount1In == 0) revert InsufficientInputAmount();
        _checkK(balance0, balance1, amount0In, amount1In, r0, r1);

        _update(balance0, balance1);
        emit Swap(msg.sender, amount0In, amount1In, amount0Out, amount1Out, to);
        if (amount0Out > 0) IERC20(token0).safeTransfer(to, amount0Out);
        if (amount1Out > 0) IERC20(token1).safeTransfer(to, amount1Out);
    }

    /// @dev The balances less the fee on what came in must keep reserve0 * reserve1 (scaled by 1000^2).
    function _checkK(uint256 balance0, uint256 balance1, uint256 amount0In, uint256 amount1In, uint112 r0, uint112 r1) private pure {
        uint256 adjusted0 = balance0 * 1000 - amount0In * FEE_PER_THOUSAND;
        uint256 adjusted1 = balance1 * 1000 - amount1In * FEE_PER_THOUSAND;
        if (adjusted0 * adjusted1 < uint256(r0) * uint256(r1) * 1_000_000) revert KInvariant();
    }

    /// @notice Sends anything above the reserves (tokens sent here by mistake) to `to`.
    function skim(address to) external nonReentrant {
        address t0 = token0;
        address t1 = token1;
        uint256 extra0 = IERC20(t0).balanceOf(address(this)) - reserve0;
        uint256 extra1 = IERC20(t1).balanceOf(address(this)) - reserve1;
        if (extra0 > 0) IERC20(t0).safeTransfer(to, extra0);
        if (extra1 > 0) IERC20(t1).safeTransfer(to, extra1);
    }

    /// @notice Sets the reserves to the balances.
    function sync() external nonReentrant {
        _update(IERC20(token0).balanceOf(address(this)), IERC20(token1).balanceOf(address(this)));
    }

    function _update(uint256 balance0, uint256 balance1) private {
        if (balance0 > type(uint112).max || balance1 > type(uint112).max) revert Overflow();
        reserve0 = uint112(balance0);
        reserve1 = uint112(balance1);
        emit Sync(reserve0, reserve1);
    }
}
