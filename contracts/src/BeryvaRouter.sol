// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {BeryvaFactory} from "./BeryvaFactory.sol";
import {BeryvaPair} from "./BeryvaPair.sol";
import {BeryvaLibrary} from "./BeryvaLibrary.sol";
import {IWETH} from "./interfaces/IWETH.sol";

/// @title Beryva Swap router
/// @notice The contract people call: it moves their tokens into the right pool and makes
///         the swap or deposit, with the two protections every call carries:
///         - a minimum to receive (`amountOutMin`, `amount*Min`), from the slippage setting, and
///         - a deadline, after which the call reverts instead of running at a stale price.
///         ETH goes through WETH (wrapped ETH): the router wraps it on the way in and unwraps
///         it on the way out.
/// @dev Uniswap V2's Router02, rewritten for Solidity 0.8, with SafeERC20 and a reentrancy
///      guard. The router never keeps tokens or ETH between calls. Left out: "exact output"
///      swaps, permit-based removals, and fee-on-transfer token support.
///      Test networks only: docs/contracts.md.
contract BeryvaRouter is ReentrancyGuard {
    using SafeERC20 for IERC20;

    address public immutable factory;
    address public immutable WETH;

    error Expired();
    error InsufficientOutputAmount();
    error InsufficientAAmount();
    error InsufficientBAmount();
    error InvalidPath();
    error OnlyWeth();
    error EthTransferFailed();
    error ZeroAddress();

    modifier ensure(uint256 deadline) {
        if (deadline < block.timestamp) revert Expired();
        _;
    }

    constructor(address factory_, address weth_) {
        if (factory_ == address(0) || weth_ == address(0)) revert ZeroAddress();
        factory = factory_;
        WETH = weth_;
    }

    /// @dev ETH only arrives from WETH, when unwrapping.
    receive() external payable {
        if (msg.sender != WETH) revert OnlyWeth();
    }

    // -----------------------------------------------------------------------
    // Liquidity

    function _addLiquidityAmounts(address tokenA, address tokenB, uint256 amountADesired, uint256 amountBDesired, uint256 amountAMin, uint256 amountBMin)
        private
        returns (uint256 amountA, uint256 amountB)
    {
        address pair = BeryvaFactory(factory).getPair(tokenA, tokenB);
        if (pair == address(0)) pair = BeryvaFactory(factory).createPair(tokenA, tokenB);
        (uint256 reserveA, uint256 reserveB) = BeryvaLibrary.getReserves(factory, tokenA, tokenB);
        if (reserveA == 0 && reserveB == 0) {
            (amountA, amountB) = (amountADesired, amountBDesired);
        } else {
            uint256 amountBOptimal = BeryvaLibrary.quote(amountADesired, reserveA, reserveB);
            if (amountBOptimal <= amountBDesired) {
                if (amountBOptimal < amountBMin) revert InsufficientBAmount();
                (amountA, amountB) = (amountADesired, amountBOptimal);
            } else {
                uint256 amountAOptimal = BeryvaLibrary.quote(amountBDesired, reserveB, reserveA);
                // amountAOptimal <= amountADesired here, since amountBOptimal > amountBDesired.
                if (amountAOptimal < amountAMin) revert InsufficientAAmount();
                (amountA, amountB) = (amountAOptimal, amountBDesired);
            }
        }
    }

    function addLiquidity(
        address tokenA,
        address tokenB,
        uint256 amountADesired,
        uint256 amountBDesired,
        uint256 amountAMin,
        uint256 amountBMin,
        address to,
        uint256 deadline
    ) external nonReentrant ensure(deadline) returns (uint256 amountA, uint256 amountB, uint256 liquidity) {
        (amountA, amountB) = _addLiquidityAmounts(tokenA, tokenB, amountADesired, amountBDesired, amountAMin, amountBMin);
        liquidity = _depositAndMint(tokenA, tokenB, amountA, amountB, to);
    }

    function _depositAndMint(address tokenA, address tokenB, uint256 amountA, uint256 amountB, address to) private returns (uint256) {
        address pair = BeryvaLibrary.pairFor(factory, tokenA, tokenB);
        IERC20(tokenA).safeTransferFrom(msg.sender, pair, amountA);
        IERC20(tokenB).safeTransferFrom(msg.sender, pair, amountB);
        return BeryvaPair(pair).mint(to);
    }

    function addLiquidityETH(address token, uint256 amountTokenDesired, uint256 amountTokenMin, uint256 amountETHMin, address to, uint256 deadline)
        external
        payable
        nonReentrant
        ensure(deadline)
        returns (uint256 amountToken, uint256 amountETH, uint256 liquidity)
    {
        (amountToken, amountETH) = _addLiquidityAmounts(token, WETH, amountTokenDesired, msg.value, amountTokenMin, amountETHMin);
        address pair = BeryvaLibrary.pairFor(factory, token, WETH);
        IERC20(token).safeTransferFrom(msg.sender, pair, amountToken);
        IWETH(WETH).deposit{value: amountETH}();
        IERC20(WETH).safeTransfer(pair, amountETH);
        liquidity = BeryvaPair(pair).mint(to);
        if (msg.value > amountETH) _sendEth(msg.sender, msg.value - amountETH);
    }

    function removeLiquidity(address tokenA, address tokenB, uint256 liquidity, uint256 amountAMin, uint256 amountBMin, address to, uint256 deadline)
        public
        nonReentrant
        ensure(deadline)
        returns (uint256 amountA, uint256 amountB)
    {
        (amountA, amountB) = _removeLiquidity(tokenA, tokenB, liquidity, amountAMin, amountBMin, to);
    }

    function removeLiquidityETH(address token, uint256 liquidity, uint256 amountTokenMin, uint256 amountETHMin, address to, uint256 deadline)
        external
        nonReentrant
        ensure(deadline)
        returns (uint256 amountToken, uint256 amountETH)
    {
        if (to == address(0)) revert ZeroAddress();
        (amountToken, amountETH) = _removeLiquidity(token, WETH, liquidity, amountTokenMin, amountETHMin, address(this));
        IERC20(token).safeTransfer(to, amountToken);
        IWETH(WETH).withdraw(amountETH);
        _sendEth(to, amountETH);
    }

    function _removeLiquidity(address tokenA, address tokenB, uint256 liquidity, uint256 amountAMin, uint256 amountBMin, address to)
        private
        returns (uint256 amountA, uint256 amountB)
    {
        address pair = BeryvaLibrary.pairFor(factory, tokenA, tokenB);
        IERC20(pair).safeTransferFrom(msg.sender, pair, liquidity);
        (uint256 amount0, uint256 amount1) = BeryvaPair(pair).burn(to);
        (address token0,) = BeryvaLibrary.sortTokens(tokenA, tokenB);
        (amountA, amountB) = tokenA == token0 ? (amount0, amount1) : (amount1, amount0);
        if (amountA < amountAMin) revert InsufficientAAmount();
        if (amountB < amountBMin) revert InsufficientBAmount();
    }

    // -----------------------------------------------------------------------
    // Swaps (exact input)

    function swapExactTokensForTokens(uint256 amountIn, uint256 amountOutMin, address[] calldata path, address to, uint256 deadline)
        external
        nonReentrant
        ensure(deadline)
        returns (uint256[] memory amounts)
    {
        amounts = BeryvaLibrary.getAmountsOut(factory, amountIn, path);
        if (amounts[amounts.length - 1] < amountOutMin) revert InsufficientOutputAmount();
        IERC20(path[0]).safeTransferFrom(msg.sender, BeryvaLibrary.pairFor(factory, path[0], path[1]), amounts[0]);
        _swap(amounts, path, to);
    }

    /// @notice Swaps all the ETH sent (msg.value) for at least `amountOutMin` of the last token in `path`.
    function swapExactETHForTokens(uint256 amountOutMin, address[] calldata path, address to, uint256 deadline)
        external
        payable
        nonReentrant
        ensure(deadline)
        returns (uint256[] memory amounts)
    {
        if (path[0] != WETH) revert InvalidPath();
        amounts = BeryvaLibrary.getAmountsOut(factory, msg.value, path);
        if (amounts[amounts.length - 1] < amountOutMin) revert InsufficientOutputAmount();
        IWETH(WETH).deposit{value: amounts[0]}();
        IERC20(WETH).safeTransfer(BeryvaLibrary.pairFor(factory, path[0], path[1]), amounts[0]);
        _swap(amounts, path, to);
    }

    /// @notice Swaps exactly `amountIn` of the first token in `path` for at least `amountOutMin` ETH.
    function swapExactTokensForETH(uint256 amountIn, uint256 amountOutMin, address[] calldata path, address to, uint256 deadline)
        external
        nonReentrant
        ensure(deadline)
        returns (uint256[] memory amounts)
    {
        if (path[path.length - 1] != WETH) revert InvalidPath();
        if (to == address(0)) revert ZeroAddress();
        amounts = BeryvaLibrary.getAmountsOut(factory, amountIn, path);
        uint256 amountOut = amounts[amounts.length - 1];
        if (amountOut < amountOutMin) revert InsufficientOutputAmount();
        IERC20(path[0]).safeTransferFrom(msg.sender, BeryvaLibrary.pairFor(factory, path[0], path[1]), amounts[0]);
        _swap(amounts, path, address(this));
        IWETH(WETH).withdraw(amountOut);
        _sendEth(to, amountOut);
    }

    /// @dev Each hop's input is already in its pool; each pool sends its output to the next pool, the last to `to`.
    function _swap(uint256[] memory amounts, address[] calldata path, address to) private {
        if (to == address(0)) revert ZeroAddress();
        for (uint256 i; i < path.length - 1; i++) {
            (address input, address output) = (path[i], path[i + 1]);
            (address token0,) = BeryvaLibrary.sortTokens(input, output);
            uint256 amountOut = amounts[i + 1];
            (uint256 amount0Out, uint256 amount1Out) = input == token0 ? (uint256(0), amountOut) : (amountOut, uint256(0));
            address recipient = i < path.length - 2 ? BeryvaLibrary.pairFor(factory, output, path[i + 2]) : to;
            BeryvaPair(BeryvaLibrary.pairFor(factory, input, output)).swap(amount0Out, amount1Out, recipient);
        }
    }

    function _sendEth(address to, uint256 value) private {
        (bool ok,) = to.call{value: value}("");
        if (!ok) revert EthTransferFailed();
    }

    // -----------------------------------------------------------------------
    // Quotes (read-only)

    function quote(uint256 amountA, uint256 reserveA, uint256 reserveB) external pure returns (uint256) {
        return BeryvaLibrary.quote(amountA, reserveA, reserveB);
    }

    function getAmountOut(uint256 amountIn, uint256 reserveIn, uint256 reserveOut) external pure returns (uint256) {
        return BeryvaLibrary.getAmountOut(amountIn, reserveIn, reserveOut);
    }

    function getAmountIn(uint256 amountOut, uint256 reserveIn, uint256 reserveOut) external pure returns (uint256) {
        return BeryvaLibrary.getAmountIn(amountOut, reserveIn, reserveOut);
    }

    function getAmountsOut(uint256 amountIn, address[] calldata path) external view returns (uint256[] memory) {
        return BeryvaLibrary.getAmountsOut(factory, amountIn, path);
    }
}
