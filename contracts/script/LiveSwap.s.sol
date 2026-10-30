// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Script, console2} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BeryvaRouter} from "../src/BeryvaRouter.sol";

/// @notice A real swap both ways on Sepolia through the deployed router, from the throwaway
///         deployer wallet: SWAP_ETH_WEI of ETH to USDC (0.5% slippage from the router's own
///         quote), then that USDC back to ETH (exact-amount approval first). Sepolia only.
///
///   ROUTER=0x... SWAP_ETH_WEI=1000000000000000 forge script script/LiveSwap.s.sol --rpc-url sepolia --broadcast
contract LiveSwap is Script {
    address internal constant WETH = 0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9;
    address internal constant USDC = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    function run() external {
        require(block.chainid == 11155111, "Sepolia only");
        uint256 key = vm.envUint("BERYVA_DEPLOYER_PRIVATE_KEY");
        BeryvaRouter router = BeryvaRouter(payable(vm.envAddress("ROUTER")));
        uint256 ethIn = vm.envUint("SWAP_ETH_WEI");
        address me = vm.addr(key);

        address[] memory toUsdc = new address[](2);
        (toUsdc[0], toUsdc[1]) = (WETH, USDC);
        address[] memory toEth = new address[](2);
        (toEth[0], toEth[1]) = (USDC, WETH);

        uint256 quotedUsdc = router.getAmountsOut(ethIn, toUsdc)[1];
        uint256 usdcBefore = IERC20(USDC).balanceOf(me);

        vm.startBroadcast(key);
        router.swapExactETHForTokens{value: ethIn}((quotedUsdc * 9950) / 10_000, toUsdc, me, block.timestamp + 20 minutes);
        vm.stopBroadcast();

        uint256 got = IERC20(USDC).balanceOf(me) - usdcBefore;
        uint256 quotedEth = router.getAmountsOut(got, toEth)[1];
        vm.startBroadcast(key);
        IERC20(USDC).approve(address(router), got);
        router.swapExactTokensForETH(got, (quotedEth * 9950) / 10_000, toEth, me, block.timestamp + 20 minutes);
        vm.stopBroadcast();

        console2.log("ETH in (wei)       ", ethIn);
        console2.log("USDC out (6 dp)    ", got);
        console2.log("ETH back, quoted   ", quotedEth);
    }
}
