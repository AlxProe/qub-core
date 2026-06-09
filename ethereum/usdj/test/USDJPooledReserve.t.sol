// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexUSD} from "../src/JinexUSD.sol";
import {USDJReserveVault} from "../src/USDJReserveVault.sol";
import {MockERC20} from "../src/MockERC20.sol";
import {USDJBridgeGateway} from "../src/USDJBridgeGateway.sol";

contract USDJPooledReserveTest {
    MockERC20 usdt;
    MockERC20 usdc;
    JinexUSD usdj;
    USDJReserveVault vault;

    constructor() {
        usdt = new MockERC20("Tether USD", "USDT");
        usdc = new MockERC20("USD Coin", "USDC");
        usdj = new JinexUSD("Jinex USD", "USDJ", address(this));
        vault = new USDJReserveVault(address(usdj), address(usdt), address(usdc), address(this));
        usdj.configureVaultMinter(address(vault));
        usdt.mint(address(this), 1_000_000e6);
        usdc.mint(address(this), 1_000_000e6);
        usdt.approve(address(vault), type(uint256).max);
        usdc.approve(address(vault), type(uint256).max);
    }

    function testInfuseUsdtMintsOneUSDJ() public {
        vault.infuse(0, 10e6, address(this));
        require(usdj.balanceOf(address(this)) == 10e6, "usdj balance");
        require(vault.usdtReserveUnits() == 10e6, "usdt reserve");
    }

    function testPooledRedemptionCanChooseOtherBucket() public {
        vault.infuse(0, 10e6, address(this));
        vault.infuse(1, 25e6, address(this));
        uint256 before = usdc.balanceOf(address(this));
        vault.melt(1, 10e6, address(this));
        require(usdc.balanceOf(address(this)) == before + 10e6, "redeemed usdc");
        require(usdj.balanceOf(address(this)) == 25e6, "remaining usdj");
        require(vault.usdcReserveUnits() == 15e6, "usdc reserve left");
    }

    function testCannotRescueReserveAssets() public {
        bool ok;
        try vault.rescueUnsupportedToken(address(usdt), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "must not rescue USDT reserve");
        try vault.rescueUnsupportedToken(address(usdc), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "must not rescue USDC reserve");
        try vault.rescueUnsupportedToken(address(usdj), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "must not rescue USDJ");
    }

    function testCannotMeltUnavailableBucket() public {
        vault.infuse(0, 10e6, address(this));
        bool ok;
        try vault.melt(1, 1e6, address(this)) { ok = true; } catch { ok = false; }
        require(!ok, "empty USDC bucket must fail");
    }
    function testBridgeToQubLocksUsdjAndAppliesOnePercentToll() public {
        usdt.approve(address(vault), 100e6);
        vault.infuse(0, 100e6, address(this));
        USDJBridgeGateway gateway = new USDJBridgeGateway(address(usdj), address(this));
        usdj.approve(address(gateway), 100e6);
        uint256 nonce = gateway.bridgeToQub(100e6, "qub1examplebridgeclaimaddress000000000000000000");
        require(nonce == 0, "nonce");
        require(usdj.balanceOf(address(gateway)) == 100e6, "gateway balance");
        require(gateway.lockedForQub() == 100e6, "locked");
        require(gateway.tollFor(100e6) == 1e6, "toll");
    }

    function testReleaseFromQubRequiresVerifier() public {
        USDJBridgeGateway gateway = new USDJBridgeGateway(address(usdj), address(this));
        try gateway.releaseFromQub(bytes32(uint256(1)), address(this), 1e6, "") {
            revert("expected verifier failure");
        } catch {}
    }

}
