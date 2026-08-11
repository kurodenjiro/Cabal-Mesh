// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CabalMeshModules} from "../CabalMeshModules.sol";

/// Test-only exposure of ERC-721's internal burn path. Production deliberately
/// publishes no burn command yet, but any future burn must continue through
/// CabalMeshModules._update so an equipped token cannot survive destruction.
contract CabalMeshModulesHarness is CabalMeshModules {
    constructor(address initialAdmin, address initialMinter, address initialRevoker)
        CabalMeshModules(initialAdmin, initialMinter, initialRevoker)
    {}

    function burnForTest(uint256 tokenId) external {
        _burn(tokenId);
    }
}
