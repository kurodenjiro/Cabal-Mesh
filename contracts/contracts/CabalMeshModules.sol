// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AccessControlDefaultAdminRules} from "@openzeppelin/contracts/access/extensions/AccessControlDefaultAdminRules.sol";
import {ERC721} from "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {Base64} from "@openzeppelin/contracts/utils/Base64.sol";
import {Strings} from "@openzeppelin/contracts/utils/Strings.sol";
import {IERC165} from "@openzeppelin/contracts/utils/introspection/IERC165.sol";
import {ICabalMeshAsset} from "./interfaces/ICabalMeshAsset.sol";
import {IERC5192} from "./interfaces/IERC5192.sol";

/// @notice Canonical authentic module and standing-badge collection.
///
/// Authentication is collection-based: clients trust this deployed contract
/// address on a specific chain, then trust only tokens issued by MINTER_ROLE.
/// Token metadata and effect fields are immutable after mint. Transferable
/// modules may occupy one operator-wallet loadout slot; standing badges are
/// ERC-5192 soulbound and can never be equipped or transferred into escrow.
contract CabalMeshModules is
    ERC721,
    AccessControlDefaultAdminRules,
    Pausable,
    ICabalMeshAsset,
    IERC5192
{
    using Strings for uint256;

    uint16 public constant SCHEMA_VERSION = 1;
    uint48 public constant ADMIN_TRANSFER_DELAY = 2 days;
    uint32 public constant MAX_RELAY_REWARD_BPS = 10_000;
    uint32 public constant MAX_PRIVACY_HOP_INCREASE = 3;
    uint32 public constant MAX_GATEWAY_SESSIONS = 32;
    uint32 public constant MAX_GATEWAY_WINDOW_KIB = 1_048_576;
    bytes32 public constant MINTER_ROLE = keccak256("CABAL_MODULE_MINTER_ROLE");
    bytes32 public constant REVOKER_ROLE = keccak256("CABAL_MODULE_REVOKER_ROLE");

    enum AssetClass {
        Module,
        StandingBadge
    }

    enum Slot {
        None,
        Radio,
        Crypto,
        Power
    }

    enum Rarity {
        Common,
        Rare,
        Epic,
        Legendary
    }

    enum EffectType {
        None,
        RelayRewardBps,
        PrivacyHopIncrease,
        GatewayLicense
    }

    /// @dev V1 parameter meanings are fixed by EffectType:
    /// RelayRewardBps=(additive bps, 0), PrivacyHopIncrease=(extra hops, 0),
    /// GatewayLicense=(max concurrent sessions, max authorized window KiB).
    struct MintSpec {
        bytes32 moduleId;
        bytes32 provenanceHash;
        string displayName;
        AssetClass assetClass;
        Slot slot;
        Rarity rarity;
        EffectType effectType;
        uint32 primaryEffectValue;
        uint32 secondaryEffectValue;
        string artworkUri;
        bytes32 artworkDigest;
    }

    struct AssetData {
        bytes32 moduleId;
        bytes32 provenanceHash;
        string displayName;
        AssetClass assetClass;
        Slot slot;
        Rarity rarity;
        EffectType effectType;
        uint32 primaryEffectValue;
        uint32 secondaryEffectValue;
        string artworkUri;
        bytes32 artworkDigest;
        uint16 schemaVersion;
        address mintedBy;
    }

    error ZeroRecipient();
    error ZeroMinter();
    error ZeroRevoker();
    error InvalidModuleId();
    error InvalidProvenance();
    error InvalidDisplayName();
    error InvalidArtwork();
    error UnsafeMetadataText();
    error InvalidAssetDefinition();
    error SoulboundToken(uint256 tokenId);
    error NotTokenOwner(uint256 tokenId);
    error NotLoadoutModule(uint256 tokenId);
    error SlotAlreadyOccupied(address operator, Slot slot, uint256 tokenId);
    error TokenAlreadyEquipped(uint256 tokenId, address operator);
    error TokenNotEquipped(uint256 tokenId);
    error InvalidRevocationReason();
    error AssetAlreadyRevoked(uint256 tokenId);

    uint256 public nextTokenId = 1;
    mapping(uint256 tokenId => AssetData data) private _assets;
    mapping(bytes32 moduleId => uint256 count) public mintedCount;
    mapping(uint256 tokenId => bool value) public revoked;

    /// The paid node identity is its operator wallet. A token can therefore
    /// affect only its current owner and only one slot for that operator.
    mapping(address operator => mapping(Slot slot => uint256 tokenId)) public equippedToken;
    mapping(uint256 tokenId => address operator) public equippedBy;

    event AssetMinted(
        uint256 indexed tokenId,
        address indexed owner,
        bytes32 indexed moduleId,
        AssetClass assetClass,
        Slot slot,
        EffectType effectType
    );
    event ModuleEquipped(uint256 indexed tokenId, address indexed operator, Slot indexed slot);
    event ModuleUnequipped(uint256 indexed tokenId, address indexed operator, Slot indexed slot);
    event AssetRevoked(uint256 indexed tokenId, bytes32 indexed reasonHash, address indexed revoker);

    constructor(address initialAdmin, address initialMinter, address initialRevoker)
        ERC721("CabalMesh Modules", "CMM")
        AccessControlDefaultAdminRules(ADMIN_TRANSFER_DELAY, initialAdmin)
    {
        if (initialMinter == address(0)) revert ZeroMinter();
        if (initialRevoker == address(0)) revert ZeroRevoker();
        _grantRole(MINTER_ROLE, initialMinter);
        _grantRole(REVOKER_ROLE, initialRevoker);
    }

    function mint(address to, MintSpec calldata spec)
        external
        onlyRole(MINTER_ROLE)
        whenNotPaused
        returns (uint256 tokenId)
    {
        if (to == address(0)) revert ZeroRecipient();
        _validateSpec(spec);

        tokenId = nextTokenId++;
        _assets[tokenId] = AssetData({
            moduleId: spec.moduleId,
            provenanceHash: spec.provenanceHash,
            displayName: spec.displayName,
            assetClass: spec.assetClass,
            slot: spec.slot,
            rarity: spec.rarity,
            effectType: spec.effectType,
            primaryEffectValue: spec.primaryEffectValue,
            secondaryEffectValue: spec.secondaryEffectValue,
            artworkUri: spec.artworkUri,
            artworkDigest: spec.artworkDigest,
            schemaVersion: SCHEMA_VERSION,
            mintedBy: msg.sender
        });
        mintedCount[spec.moduleId] += 1;
        _safeMint(to, tokenId);

        if (spec.assetClass == AssetClass.StandingBadge) emit Locked(tokenId);
        else emit Unlocked(tokenId);
        emit AssetMinted(tokenId, to, spec.moduleId, spec.assetClass, spec.slot, spec.effectType);
    }

    function pauseMinting() external onlyRole(DEFAULT_ADMIN_ROLE) {
        _pause();
    }

    function unpauseMinting() external onlyRole(DEFAULT_ADMIN_ROLE) {
        _unpause();
    }

    /// @notice Irreversibly quarantines a compromised or incorrectly issued
    /// token. The NFT remains visible and directly transferable as evidence,
    /// but cannot equip or enter a new official marketplace escrow.
    function revoke(uint256 tokenId, bytes32 reasonHash) external onlyRole(REVOKER_ROLE) {
        ownerOf(tokenId);
        if (reasonHash == bytes32(0)) revert InvalidRevocationReason();
        if (revoked[tokenId]) revert AssetAlreadyRevoked(tokenId);

        revoked[tokenId] = true;
        address operator = equippedBy[tokenId];
        if (operator != address(0)) _clearLoadout(tokenId, operator);
        emit AssetRevoked(tokenId, reasonHash, msg.sender);
    }

    function equip(uint256 tokenId) external {
        address owner = ownerOf(tokenId);
        if (owner != msg.sender) revert NotTokenOwner(tokenId);

        AssetData storage data = _assets[tokenId];
        if (revoked[tokenId] || data.assetClass != AssetClass.Module || data.slot == Slot.None) {
            revert NotLoadoutModule(tokenId);
        }
        address currentOperator = equippedBy[tokenId];
        if (currentOperator != address(0)) {
            revert TokenAlreadyEquipped(tokenId, currentOperator);
        }
        uint256 occupyingToken = equippedToken[msg.sender][data.slot];
        if (occupyingToken != 0) {
            revert SlotAlreadyOccupied(msg.sender, data.slot, occupyingToken);
        }

        equippedBy[tokenId] = msg.sender;
        equippedToken[msg.sender][data.slot] = tokenId;
        emit ModuleEquipped(tokenId, msg.sender, data.slot);
    }

    function unequip(uint256 tokenId) external {
        if (ownerOf(tokenId) != msg.sender) revert NotTokenOwner(tokenId);
        if (equippedBy[tokenId] != msg.sender) revert TokenNotEquipped(tokenId);
        _clearLoadout(tokenId, msg.sender);
    }

    function locked(uint256 tokenId) public view override returns (bool) {
        _requireOwned(tokenId);
        return _assets[tokenId].assetClass == AssetClass.StandingBadge;
    }

    function isMarketplaceEligible(uint256 tokenId) external view override returns (bool) {
        return !locked(tokenId) && !revoked[tokenId];
    }

    function assetData(uint256 tokenId) external view returns (AssetData memory) {
        _requireOwned(tokenId);
        return _assets[tokenId];
    }

    /// @notice Standards-compatible ERC-721 metadata as an immutable on-chain
    /// data URI. Only artwork resolution depends on the content-addressed URI.
    function tokenURI(uint256 tokenId) public view override returns (string memory) {
        _requireOwned(tokenId);
        AssetData storage data = _assets[tokenId];

        bytes memory header = abi.encodePacked(
            '{"name":"',
            data.displayName,
            '","description":"Authentic CabalMesh ',
            _assetClassName(data.assetClass),
            '","image":"',
            data.artworkUri,
            '","attributes":'
        );
        bytes memory json = abi.encodePacked(
            header, _attributes(data), ',"cabalmesh":', _cabalMeshMetadata(data), "}"
        );
        return string.concat("data:application/json;base64,", Base64.encode(json));
    }

    function supportsInterface(bytes4 interfaceId)
        public
        view
        override(ERC721, AccessControlDefaultAdminRules, IERC165)
        returns (bool)
    {
        return interfaceId == type(ICabalMeshAsset).interfaceId
            || interfaceId == type(IERC5192).interfaceId || super.supportsInterface(interfaceId);
    }

    function _update(address to, uint256 tokenId, address auth)
        internal
        override
        returns (address from)
    {
        from = _ownerOf(tokenId);
        if (from != address(0) && to != address(0) && locked(tokenId)) {
            revert SoulboundToken(tokenId);
        }
        if (from != address(0) && from != to && equippedBy[tokenId] != address(0)) {
            _clearLoadout(tokenId, from);
        }
        return super._update(to, tokenId, auth);
    }

    function _clearLoadout(uint256 tokenId, address operator) private {
        Slot slot = _assets[tokenId].slot;
        delete equippedBy[tokenId];
        delete equippedToken[operator][slot];
        emit ModuleUnequipped(tokenId, operator, slot);
    }

    function _validateSpec(MintSpec calldata spec) private pure {
        if (spec.moduleId == bytes32(0)) revert InvalidModuleId();
        if (spec.provenanceHash == bytes32(0)) revert InvalidProvenance();
        bytes memory displayName = bytes(spec.displayName);
        if (displayName.length == 0 || displayName.length > 80) revert InvalidDisplayName();
        bytes memory artworkUri = bytes(spec.artworkUri);
        if (artworkUri.length < 8 || artworkUri.length > 200 || !_startsWithIpfs(artworkUri)) {
            revert InvalidArtwork();
        }
        if (spec.artworkDigest == bytes32(0)) revert InvalidArtwork();
        _requireJsonSafe(displayName);
        _requireJsonSafe(artworkUri);

        if (spec.assetClass == AssetClass.StandingBadge) {
            if (
                spec.slot != Slot.None || spec.effectType != EffectType.None
                    || spec.primaryEffectValue != 0 || spec.secondaryEffectValue != 0
            ) revert InvalidAssetDefinition();
            return;
        }

        if (spec.slot == Slot.Radio && spec.effectType == EffectType.RelayRewardBps) {
            if (
                spec.primaryEffectValue == 0 || spec.primaryEffectValue > MAX_RELAY_REWARD_BPS
                    || spec.secondaryEffectValue != 0
            ) revert InvalidAssetDefinition();
            return;
        }
        if (spec.slot == Slot.Crypto && spec.effectType == EffectType.PrivacyHopIncrease) {
            if (
                spec.primaryEffectValue == 0
                    || spec.primaryEffectValue > MAX_PRIVACY_HOP_INCREASE
                    || spec.secondaryEffectValue != 0
            ) revert InvalidAssetDefinition();
            return;
        }
        if (spec.slot == Slot.Power && spec.effectType == EffectType.GatewayLicense) {
            if (
                spec.primaryEffectValue == 0 || spec.primaryEffectValue > MAX_GATEWAY_SESSIONS
                    || spec.secondaryEffectValue == 0
                    || spec.secondaryEffectValue > MAX_GATEWAY_WINDOW_KIB
            ) revert InvalidAssetDefinition();
            return;
        }
        revert InvalidAssetDefinition();
    }

    function _startsWithIpfs(bytes memory value) private pure returns (bool) {
        return value[0] == "i" && value[1] == "p" && value[2] == "f" && value[3] == "s"
            && value[4] == ":" && value[5] == "/" && value[6] == "/";
    }

    function _requireJsonSafe(bytes memory value) private pure {
        for (uint256 i = 0; i < value.length; ++i) {
            bytes1 character = value[i];
            if (
                character == 0x22 || character == 0x5c || uint8(character) < 0x20
                    || uint8(character) > 0x7e
            ) {
                revert UnsafeMetadataText();
            }
        }
    }

    function _attributes(AssetData storage data) private view returns (bytes memory) {
        bytes memory identity = abi.encodePacked(
            '{"trait_type":"Asset Class","value":"',
            _assetClassName(data.assetClass),
            '"},{"trait_type":"Slot","value":"',
            _slotName(data.slot),
            '"},{"trait_type":"Rarity","value":"',
            _rarityName(data.rarity),
            '"}'
        );
        bytes memory effect = abi.encodePacked(
            '{"trait_type":"Effect Type","value":"',
            _effectName(data.effectType),
            '"},{"display_type":"number","trait_type":"Primary Effect Value","value":',
            uint256(data.primaryEffectValue).toString(),
            '},{"display_type":"number","trait_type":"Secondary Effect Value","value":',
            uint256(data.secondaryEffectValue).toString(),
            "}"
        );
        bytes memory schema = abi.encodePacked(
            '{"display_type":"number","trait_type":"Schema Version","value":',
            uint256(data.schemaVersion).toString(),
            "}"
        );
        return abi.encodePacked("[", identity, ",", effect, ",", schema, "]");
    }

    function _cabalMeshMetadata(AssetData storage data) private view returns (bytes memory) {
        bytes memory identity = abi.encodePacked(
            '"schema_version":',
            uint256(data.schemaVersion).toString(),
            ',"module_id":"',
            uint256(data.moduleId).toHexString(32),
            '","provenance_hash":"',
            uint256(data.provenanceHash).toHexString(32),
            '","asset_class":"',
            _assetClassName(data.assetClass),
            '","slot":"',
            _slotName(data.slot),
            '","rarity":"',
            _rarityName(data.rarity),
            '"'
        );
        bytes memory effect = abi.encodePacked(
            '"effect":{"type":"',
            _effectName(data.effectType),
            '","primary":',
            uint256(data.primaryEffectValue).toString(),
            ',"secondary":',
            uint256(data.secondaryEffectValue).toString(),
            "}"
        );
        bytes memory artwork = abi.encodePacked(
            '"artwork_uri":"',
            data.artworkUri,
            '","artwork_digest":"',
            uint256(data.artworkDigest).toHexString(32),
            '"'
        );
        return abi.encodePacked("{", identity, ",", effect, ",", artwork, "}");
    }

    function _assetClassName(AssetClass value) private pure returns (string memory) {
        return value == AssetClass.Module ? "MODULE" : "STANDING_BADGE";
    }

    function _slotName(Slot value) private pure returns (string memory) {
        if (value == Slot.Radio) return "RADIO";
        if (value == Slot.Crypto) return "CRYPTO";
        if (value == Slot.Power) return "POWER";
        return "NONE";
    }

    function _rarityName(Rarity value) private pure returns (string memory) {
        if (value == Rarity.Rare) return "RARE";
        if (value == Rarity.Epic) return "EPIC";
        if (value == Rarity.Legendary) return "LEGENDARY";
        return "COMMON";
    }

    function _effectName(EffectType value) private pure returns (string memory) {
        if (value == EffectType.RelayRewardBps) return "RELAY_REWARD_BPS";
        if (value == EffectType.PrivacyHopIncrease) return "PRIVACY_HOP_INCREASE";
        if (value == EffectType.GatewayLicense) return "GATEWAY_LICENSE";
        return "NONE";
    }
}
