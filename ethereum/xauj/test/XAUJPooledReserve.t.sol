// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexGold} from "../src/JinexGold.sol";
import {XAUJReserveVault} from "../src/XAUJReserveVault.sol";
import {MockERC20} from "../src/MockERC20.sol";

contract XAUJPooledReserveTest {
    MockERC20 paxg;
    MockERC20 xaut;
    JinexGold xauj;
    XAUJReserveVault vault;

    constructor() {
        paxg = new MockERC20("Mock PAX Gold", "PAXG", 18);
        xaut = new MockERC20("Mock Tether Gold", "XAUt", 6);
        xauj = new JinexGold("Jinex Gold", "XAUJ", address(this));
        vault = new XAUJReserveVault(address(xauj), address(paxg), address(xaut), address(this));
        xauj.configureVaultMinter(address(vault));
        paxg.mint(address(this), 1_000_000e18);
        xaut.mint(address(this), 1_000_000e6);
        paxg.approve(address(vault), type(uint256).max);
        xaut.approve(address(vault), type(uint256).max);
    }

    function testInfusePaxgMintsOneXAUJ() public {
        vault.infuse(0, 10e18, address(this));
        require(xauj.balanceOf(address(this)) == 10e18, "xauj balance");
        require(vault.paxgReserveUnits() == 10e18, "paxg reserve");
    }

    function testPooledRedemptionCanChooseOtherBucket() public {
        vault.infuse(0, 10e18, address(this));
        vault.infuse(1, 25e6, address(this));
        uint256 before = xaut.balanceOf(address(this));
        vault.melt(1, 10e18, address(this));
        require(xaut.balanceOf(address(this)) == before + 10e6, "redeemed xaut");
        require(xauj.balanceOf(address(this)) == 25e18, "remaining xauj");
        require(vault.xautReserveUnits() == 15e6, "xaut reserve left");
    }

    function testCannotMeltUnavailableBucket() public {
        vault.infuse(0, 10e18, address(this));
        bool ok;
        try vault.melt(1, 1e18, address(this)) { ok = true; } catch { ok = false; }
        require(!ok, "empty XAUt bucket must fail");
    }

    function testCannotMeltSubMicroXAUJToXAUT() public {
        vault.infuse(1, 25e6, address(this));
        bool ok;
        try vault.melt(1, 1, address(this)) { ok = true; } catch { ok = false; }
        require(!ok, "sub-micro XAUJ melt to XAUt must fail");
    }

    function testCannotRescueReserveAssets() public {
        bool ok;
        try vault.rescueUnsupportedToken(address(paxg), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "cannot rescue PAXG reserve");
        try vault.rescueUnsupportedToken(address(xaut), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "cannot rescue XAUt reserve");
        try vault.rescueUnsupportedToken(address(xauj), address(this), 1) { ok = true; } catch { ok = false; }
        require(!ok, "cannot rescue XAUJ");
    }
}
