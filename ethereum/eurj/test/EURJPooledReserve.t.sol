// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexEUR} from "../src/JinexEUR.sol";
import {EURJReserveVault} from "../src/EURJReserveVault.sol";
import {MockERC20} from "../src/MockERC20.sol";

contract EURJPooledReserveTest {
    MockERC20 eurc;
    MockERC20 eurs;
    JinexEUR eurj;
    EURJReserveVault vault;

    constructor() {
        eurc = new MockERC20("Mock EUR Coin", "EURC", 6);
        eurs = new MockERC20("Mock STASIS EURS", "EURS", 2);
        eurj = new JinexEUR("Jinex EUR", "EURJ", address(this));
        vault = new EURJReserveVault(address(eurj), address(eurc), address(eurs), address(this));
        eurj.configureVaultMinter(address(vault));
        eurc.mint(address(this), 1_000_000e6);
        eurs.mint(address(this), 1_000_000e2);
        eurc.approve(address(vault), type(uint256).max);
        eurs.approve(address(vault), type(uint256).max);
    }

    function testInfuseEurcMintsOneEURJ() public {
        vault.infuse(0, 10e6, address(this));
        require(eurj.balanceOf(address(this)) == 10e6, "eurj balance");
        require(vault.eurcReserveUnits() == 10e6, "eurc reserve");
    }

    function testPooledRedemptionCanChooseOtherBucket() public {
        vault.infuse(0, 10e6, address(this));
        vault.infuse(1, 25e2, address(this));
        uint256 before = eurs.balanceOf(address(this));
        vault.melt(1, 10e6, address(this));
        require(eurs.balanceOf(address(this)) == before + 10e2, "redeemed eurs");
        require(eurj.balanceOf(address(this)) == 25e6, "remaining eurj");
        require(vault.eursReserveUnits() == 15e2, "eurs reserve left");
    }

    function testCannotMeltUnavailableBucket() public {
        vault.infuse(0, 10e6, address(this));
        bool ok;
        try vault.melt(1, 1e6, address(this)) { ok = true; } catch { ok = false; }
        require(!ok, "empty EURS bucket must fail");
    }
    function testCannotMeltSubCentEURSToEURS() public {
        vault.infuse(1, 25e2, address(this));
        bool ok;
        try vault.melt(1, 1, address(this)) { ok = true; } catch { ok = false; }
        require(!ok, "sub-cent EURJ melt to EURS must fail");
    }

}
