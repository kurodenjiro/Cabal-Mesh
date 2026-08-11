// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC165} from "@openzeppelin/contracts/utils/introspection/IERC165.sol";

/// @notice Discovery hook used by CabalMesh marketplaces before accepting an
/// asset. Collections that implement this interface state official marketplace
/// eligibility rather than relying on a transfer to fail after a buyer pays.
interface ICabalMeshAsset is IERC165 {
    function isMarketplaceEligible(uint256 tokenId) external view returns (bool);
}
