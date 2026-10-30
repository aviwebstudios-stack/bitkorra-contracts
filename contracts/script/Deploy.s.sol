// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Script, console2} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BeryvaFactory} from "../src/BeryvaFactory.sol";
import {BeryvaRouter} from "../src/BeryvaRouter.sol";

/// @notice Deploys Beryva Swap on Sepolia and seeds the ETH/USDC pool.
///
/// Sepolia only: the script stops on any other chain. The deployer key comes from
/// BERYVA_DEPLOYER_PRIVATE_KEY (server-only, in .env.local, never committed). The seed
/// comes from SEED_ETH_WEI and SEED_USDC (6 decimals), so the pool's opening price is
/// SEED_USDC / SEED_ETH. Run from contracts/ (docs/contracts.md):
///
///   forge script script/Deploy.s.sol --rpc-url sepolia --broadcast
contract Deploy is Script {
    uint256 internal constant SEPOLIA = 11155111;
    /// @dev WETH9 on Sepolia (the one Uniswap's Sepolia deployment uses).
    address internal constant WETH = 0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9;
    /// @dev Circle's test USDC on Sepolia (lib/beryva/assets.ts).
    address internal constant USDC = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    function run() external returns (BeryvaFactory factory, BeryvaRouter router, address pair) {
        require(block.chainid == SEPOLIA, "Beryva Swap deploys to Sepolia only");
        uint256 key = vm.envUint("BERYVA_DEPLOYER_PRIVATE_KEY");
        uint256 seedEth = vm.envUint("SEED_ETH_WEI");
        uint256 seedUsdc = vm.envUint("SEED_USDC");
        address deployer = vm.addr(key);
        require(IERC20(USDC).balanceOf(deployer) >= seedUsdc, "Not enough test USDC for the seed");
        require(deployer.balance > seedEth, "Not enough Sepolia ETH for the seed and gas");

        vm.startBroadcast(key);
        factory = new BeryvaFactory();
        router = new BeryvaRouter(address(factory), WETH);
        IERC20(USDC).approve(address(router), seedUsdc);
        router.addLiquidityETH{value: seedEth}(USDC, seedUsdc, seedUsdc, seedEth, deployer, block.timestamp + 15 minutes);
        vm.stopBroadcast();

        pair = factory.getPair(WETH, USDC);
        console2.log("BeryvaFactory", address(factory));
        console2.log("BeryvaRouter ", address(router));
        console2.log("ETH/USDC pool", pair);
    }
}
