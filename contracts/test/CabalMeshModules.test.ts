import { expect } from "chai";
import { ethers } from "hardhat";
import { time } from "@nomicfoundation/hardhat-network-helpers";

const AssetClass = { Module: 0, StandingBadge: 1 } as const;
const Slot = { None: 0, Radio: 1, Crypto: 2, Power: 3 } as const;
const Rarity = { Common: 0, Rare: 1, Epic: 2, Legendary: 3 } as const;
const EffectType = {
  None: 0,
  RelayRewardBps: 1,
  PrivacyHopIncrease: 2,
  GatewayLicense: 3,
} as const;

describe("CabalMeshModules", function () {
  let milestoneSequence = 0;
  async function deployFixture() {
    const [admin, minter, owner, buyer, other] = await ethers.getSigners();
    const Modules = await ethers.getContractFactory("CabalMeshModules");
    const modules = await Modules.deploy(admin.address, minter.address, admin.address);
    await modules.waitForDeployment();
    return { modules, admin, minter, owner, buyer, other };
  }

  function artworkDigest(label: string) {
    return ethers.sha256(ethers.toUtf8Bytes(label));
  }

  function radioSpec(overrides: Record<string, unknown> = {}) {
    return {
      moduleId: ethers.id("cabalmesh.radio.amplifier-mk2.v1"),
      provenanceHash: ethers.id("fuji-settlement-radio-vector-1"),
      displayName: "Relay Amplifier MK-II",
      assetClass: AssetClass.Module,
      slot: Slot.Radio,
      rarity: Rarity.Rare,
      effectType: EffectType.RelayRewardBps,
      primaryEffectValue: 1_800,
      secondaryEffectValue: 0,
      artworkUri: "ipfs://bafybeiradioamplifiermk2",
      artworkDigest: artworkDigest("radio-amplifier-mk2-artwork"),
      ...overrides,
    };
  }

  function cryptoSpec(overrides: Record<string, unknown> = {}) {
    return {
      moduleId: ethers.id("cabalmesh.crypto.ghost-cloak.v1"),
      provenanceHash: ethers.id("fuji-settlement-crypto-vector-1"),
      displayName: "Ghost Cloak",
      assetClass: AssetClass.Module,
      slot: Slot.Crypto,
      rarity: Rarity.Epic,
      effectType: EffectType.PrivacyHopIncrease,
      primaryEffectValue: 2,
      secondaryEffectValue: 0,
      artworkUri: "ipfs://bafybeighostcloak",
      artworkDigest: artworkDigest("ghost-cloak-artwork"),
      ...overrides,
    };
  }

  function badgeSpec(overrides: Record<string, unknown> = {}) {
    return {
      moduleId: ethers.id("cabalmesh.standing.first-ten-settlements.v1"),
      provenanceHash: ethers.id("fuji-standing-vector-1"),
      displayName: "First Ten Settlements",
      assetClass: AssetClass.StandingBadge,
      slot: Slot.None,
      rarity: Rarity.Common,
      effectType: EffectType.None,
      primaryEffectValue: 0,
      secondaryEffectValue: 0,
      artworkUri: "ipfs://bafybeifirsttensettlements",
      artworkDigest: artworkDigest("first-ten-settlements-artwork"),
      ...overrides,
    };
  }

  async function mint(modules: any, minter: any, to: string, spec?: any) {
    const award = spec ?? radioSpec({
      provenanceHash: ethers.id(`test-milestone-${++milestoneSequence}`),
    });
    const tokenId = await modules.nextTokenId();
    await modules.connect(minter).awardMilestone(to, award);
    return tokenId;
  }

  describe("authority and immutable definitions", function () {
    it("keeps deployed bytecode below the EVM contract-size limit", async function () {
      const { modules } = await deployFixture();
      const runtimeCode = await ethers.provider.getCode(await modules.getAddress());

      expect((runtimeCode.length - 2) / 2).to.be.lessThanOrEqual(24_576);
    });

    it("separates delayed administration from mint authority", async function () {
      const { modules, admin, minter, owner } = await deployFixture();

      expect(await modules.defaultAdmin()).to.equal(admin.address);
      expect(await modules.defaultAdminDelay()).to.equal(2 * 24 * 60 * 60);
      expect(await modules.hasRole(await modules.MINTER_ROLE(), minter.address)).to.equal(true);
      expect(await modules.hasRole(await modules.REVOKER_ROLE(), admin.address)).to.equal(true);
      expect(await modules.hasRole(await modules.MINTER_ROLE(), owner.address)).to.equal(false);
    });

    it("enforces the two-step delay before a new default admin can accept", async function () {
      const { modules, admin, other } = await deployFixture();
      await modules.connect(admin).beginDefaultAdminTransfer(other.address);
      const [, acceptSchedule] = await modules.pendingDefaultAdmin();

      await expect(modules.connect(other).acceptDefaultAdminTransfer())
        .to.be.revertedWithCustomError(modules, "AccessControlEnforcedDefaultAdminDelay")
        .withArgs(acceptSchedule);
      await time.increase(2 * 24 * 60 * 60);
      await modules.connect(other).acceptDefaultAdminTransfer();

      expect(await modules.defaultAdmin()).to.equal(other.address);
      expect(await modules.hasRole(await modules.DEFAULT_ADMIN_ROLE(), admin.address)).to.equal(false);
    });

    it("prevents an ordinary wallet from minting a reward-bearing module", async function () {
      const { modules, owner } = await deployFixture();
      const minterRole = await modules.MINTER_ROLE();

      await expect(modules.connect(owner).awardMilestone(owner.address, radioSpec()))
        .to.be.revertedWithCustomError(modules, "AccessControlUnauthorizedAccount")
        .withArgs(owner.address, minterRole);
    });

    it("consumes one milestone provenance exactly once", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const spec = radioSpec({ provenanceHash: ethers.id("verified-settlement-42") });

      await expect(modules.connect(minter).awardMilestone(owner.address, spec))
        .to.emit(modules, "AssetMinted")
        .withArgs(1, owner.address, spec.moduleId, AssetClass.Module, Slot.Radio, EffectType.RelayRewardBps);
      expect(await modules.tokenForProvenance(spec.provenanceHash)).to.equal(1);

      // Competing submissions are serialized by the EVM; once either consumes
      // the commitment, every replay observes the same non-zero token id.
      await expect(modules.connect(minter).awardMilestone(buyer.address, spec))
        .to.be.revertedWithCustomError(modules, "MilestoneAlreadyAwarded")
        .withArgs(spec.provenanceHash, 1);
      expect(await modules.balanceOf(owner.address)).to.equal(1);
      expect(await modules.balanceOf(buyer.address)).to.equal(0);
      expect(await modules.nextTokenId()).to.equal(2);
    });

    it("lets only the admin grant or revoke a minter", async function () {
      const { modules, admin, minter, owner, other } = await deployFixture();
      const minterRole = await modules.MINTER_ROLE();

      await expect(modules.connect(other).grantRole(minterRole, owner.address))
        .to.be.revertedWithCustomError(modules, "AccessControlUnauthorizedAccount");
      await modules.connect(admin).grantRole(minterRole, owner.address);
      await expect(modules.connect(owner).awardMilestone(owner.address, cryptoSpec())).to.emit(
        modules,
        "AssetMinted"
      );
      await modules.connect(admin).revokeRole(minterRole, owner.address);
      await expect(modules.connect(owner).awardMilestone(owner.address, cryptoSpec()))
        .to.be.revertedWithCustomError(modules, "AccessControlUnauthorizedAccount");

      expect(await modules.hasRole(minterRole, minter.address)).to.equal(true);
    });

    it("lets the admin stop new issuance without freezing holder transfers", async function () {
      const { modules, admin, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);

      await modules.connect(admin).pauseMinting();
      await expect(modules.connect(minter).awardMilestone(owner.address, radioSpec({
        provenanceHash: ethers.id("paused-milestone"),
      })))
        .to.be.revertedWithCustomError(modules, "EnforcedPause");
      await modules.connect(owner).transferFrom(owner.address, buyer.address, tokenId);
      expect(await modules.ownerOf(tokenId)).to.equal(buyer.address);

      await modules.connect(admin).unpauseMinting();
      await expect(modules.connect(minter).awardMilestone(owner.address, radioSpec({
        provenanceHash: ethers.id("unpaused-milestone"),
      }))).to.emit(
        modules,
        "AssetMinted"
      );
    });

    it("lets a separate revoker irreversibly quarantine a compromised issue", async function () {
      const { modules, admin, minter, owner, buyer, other } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      const metadataBefore = await modules.tokenURI(tokenId);
      const Marketplace = await ethers.getContractFactory("Marketplace");
      const marketplace = await Marketplace.deploy(await modules.getAddress(), 3 * 24 * 60 * 60);
      await marketplace.waitForDeployment();
      await modules.connect(owner).equip(tokenId);
      await modules.connect(owner).approve(await marketplace.getAddress(), tokenId);
      await marketplace.connect(owner).createListing("Relay Amplifier", 100, tokenId);
      const reasonHash = ethers.id("incident-2026-08-example");

      await expect(modules.connect(other).revoke(tokenId, reasonHash))
        .to.be.revertedWithCustomError(modules, "AccessControlUnauthorizedAccount");
      await expect(modules.connect(admin).revoke(tokenId, reasonHash))
        .to.emit(modules, "AssetRevoked")
        .withArgs(tokenId, reasonHash, admin.address);

      expect(await modules.revoked(tokenId)).to.equal(true);
      expect(await modules.locked(tokenId)).to.equal(false);
      expect(await modules.isMarketplaceEligible(tokenId)).to.equal(false);
      expect(await modules.equippedBy(tokenId)).to.equal(ethers.ZeroAddress);
      expect(await modules.tokenURI(tokenId)).to.equal(metadataBefore);
      await expect(marketplace.connect(buyer).buy(1, { value: 100 }))
        .to.be.revertedWithCustomError(marketplace, "IneligibleAsset")
        .withArgs(await modules.getAddress(), tokenId);
      expect((await marketplace.listings(1)).active).to.equal(true);
      expect(await modules.ownerOf(tokenId)).to.equal(owner.address);
      await expect(modules.connect(admin).revoke(tokenId, reasonHash))
        .to.be.revertedWithCustomError(modules, "AssetAlreadyRevoked")
        .withArgs(tokenId);
    });

    it("stores a complete structured v1 definition and issuer provenance", async function () {
      const { modules, minter, owner } = await deployFixture();
      const spec = radioSpec();
      const tokenId = await mint(modules, minter, owner.address, spec);

      const data = await modules.assetData(tokenId);
      expect(data.moduleId).to.equal(spec.moduleId);
      expect(data.provenanceHash).to.equal(spec.provenanceHash);
      expect(data.displayName).to.equal(spec.displayName);
      expect(data.assetClass).to.equal(AssetClass.Module);
      expect(data.slot).to.equal(Slot.Radio);
      expect(data.rarity).to.equal(Rarity.Rare);
      expect(data.effectType).to.equal(EffectType.RelayRewardBps);
      expect(data.primaryEffectValue).to.equal(1_800);
      expect(data.secondaryEffectValue).to.equal(0);
      expect(data.artworkUri).to.equal(spec.artworkUri);
      expect(data.artworkDigest).to.equal(spec.artworkDigest);
      expect(data.schemaVersion).to.equal(1);
      expect(data.mintedBy).to.equal(minter.address);
      expect(await modules.mintedCount(spec.moduleId)).to.equal(1);
    });

    it("keeps token metadata unchanged across ownership transfers", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      const before = await modules.tokenURI(tokenId);
      const dataBefore = await modules.assetData(tokenId);

      await modules.connect(owner).transferFrom(owner.address, buyer.address, tokenId);

      expect(await modules.tokenURI(tokenId)).to.equal(before);
      expect(await modules.assetData(tokenId)).to.deep.equal(dataBefore);
    });

    it("enumerates only tokens in the current owner's balance", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const first = await mint(modules, minter, owner.address);
      const second = await mint(modules, minter, owner.address);

      expect(await modules.balanceOf(owner.address)).to.equal(2);
      expect([
        await modules.tokenOfOwnerByIndex(owner.address, 0),
        await modules.tokenOfOwnerByIndex(owner.address, 1),
      ]).to.have.members([first, second]);

      await modules.connect(owner).transferFrom(owner.address, buyer.address, first);
      expect(await modules.balanceOf(owner.address)).to.equal(1);
      expect(await modules.tokenOfOwnerByIndex(owner.address, 0)).to.equal(second);
      expect(await modules.tokenOfOwnerByIndex(buyer.address, 0)).to.equal(first);
    });
  });

  describe("metadata interoperability", function () {
    it("returns ERC-721 JSON metadata as an on-chain data URI", async function () {
      const { modules, minter, owner } = await deployFixture();
      const spec = radioSpec();
      const tokenId = await mint(modules, minter, owner.address, spec);

      const uri: string = await modules.tokenURI(tokenId);
      const prefix = "data:application/json;base64,";
      expect(uri.startsWith(prefix)).to.equal(true);
      const metadata = JSON.parse(Buffer.from(uri.slice(prefix.length), "base64").toString("utf8"));

      expect(metadata.name).to.equal(spec.displayName);
      expect(metadata.description).to.equal("Authentic CabalMesh MODULE");
      expect(metadata.image).to.equal(spec.artworkUri);
      expect(metadata.cabalmesh).to.deep.include({
        schema_version: 1,
        module_id: spec.moduleId,
        provenance_hash: spec.provenanceHash,
        asset_class: "MODULE",
        slot: "RADIO",
        rarity: "RARE",
      });
      expect(metadata.cabalmesh.effect).to.deep.equal({
        type: "RELAY_REWARD_BPS",
        primary: 1_800,
        secondary: 0,
      });
      expect(metadata.cabalmesh.artwork_digest).to.equal(spec.artworkDigest);
      expect(metadata.attributes).to.deep.include({ trait_type: "Slot", value: "RADIO" });
    });

    it("advertises ERC-721 metadata, ERC-5192, and marketplace eligibility", async function () {
      const { modules } = await deployFixture();
      const marketplaceEligibilitySelector = ethers.id("isMarketplaceEligible(uint256)").slice(0, 10);

      expect(await modules.supportsInterface("0x80ac58cd")).to.equal(true);
      expect(await modules.supportsInterface("0x5b5e139f")).to.equal(true);
      expect(await modules.supportsInterface("0x780e9d63")).to.equal(true);
      expect(await modules.supportsInterface("0xb45a3c0e")).to.equal(true);
      expect(await modules.supportsInterface(marketplaceEligibilitySelector)).to.equal(true);
    });

    it("rejects malformed, mutable-looking, or mismatched definitions", async function () {
      const { modules, minter, owner } = await deployFixture();
      const cases: Array<[Record<string, unknown>, string]> = [
        [{ moduleId: ethers.ZeroHash }, "InvalidModuleId"],
        [{ provenanceHash: ethers.ZeroHash }, "InvalidProvenance"],
        [{ displayName: "" }, "InvalidDisplayName"],
        [{ displayName: 'Bad \" JSON' }, "UnsafeMetadataText"],
        [{ displayName: "Rádio Module" }, "UnsafeMetadataText"],
        [{ artworkUri: "https://mutable.example/art.png" }, "InvalidArtwork"],
        [{ artworkDigest: ethers.ZeroHash }, "InvalidArtwork"],
        [{ slot: Slot.Crypto }, "InvalidAssetDefinition"],
        [{ primaryEffectValue: 10_001 }, "InvalidAssetDefinition"],
        [{ secondaryEffectValue: 1 }, "InvalidAssetDefinition"],
      ];

      for (const [overrides, error] of cases) {
        await expect(modules.connect(minter).awardMilestone(owner.address, radioSpec(overrides)))
          .to.be.revertedWithCustomError(modules, error);
      }
      await expect(
        modules.connect(minter).awardMilestone(
          owner.address,
          badgeSpec({ slot: Slot.Radio, effectType: EffectType.RelayRewardBps })
        )
      ).to.be.revertedWithCustomError(modules, "InvalidAssetDefinition");
    });
  });

  describe("standing badges", function () {
    it("mints a discoverably locked ERC-5192 badge", async function () {
      const { modules, minter, owner } = await deployFixture();
      const tokenId = await modules.nextTokenId();

      await expect(modules.connect(minter).awardMilestone(owner.address, badgeSpec()))
        .to.emit(modules, "Locked")
        .withArgs(tokenId);

      expect(await modules.locked(tokenId)).to.equal(true);
      expect(await modules.isMarketplaceEligible(tokenId)).to.equal(false);
    });

    it("rejects every badge transfer and loadout attempt", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address, badgeSpec());

      await expect(modules.connect(owner).transferFrom(owner.address, buyer.address, tokenId))
        .to.be.revertedWithCustomError(modules, "SoulboundToken")
        .withArgs(tokenId);
      await expect(modules.connect(owner).equip(tokenId))
        .to.be.revertedWithCustomError(modules, "NotLoadoutModule")
        .withArgs(tokenId);
      expect(await modules.ownerOf(tokenId)).to.equal(owner.address);
    });

    it("rejects token approval for a soulbound badge", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address, badgeSpec());

      await expect(modules.connect(owner).approve(buyer.address, tokenId))
        .to.be.revertedWithCustomError(modules, "SoulboundToken")
        .withArgs(tokenId);
      expect(await modules.getApproved(tokenId)).to.equal(ethers.ZeroAddress);
    });

    it("rejects a badge at marketplace listing before AVAX or escrow can move", async function () {
      const { modules, minter, owner } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address, badgeSpec());
      const Marketplace = await ethers.getContractFactory("Marketplace");
      const marketplace = await Marketplace.deploy(await modules.getAddress(), 3 * 24 * 60 * 60);
      await marketplace.waitForDeployment();
      await modules.connect(owner).setApprovalForAll(await marketplace.getAddress(), true);

      await expect(marketplace.connect(owner).createListing("badge", 1, tokenId))
        .to.be.revertedWithCustomError(marketplace, "IneligibleAsset")
        .withArgs(await modules.getAddress(), tokenId);
      expect(await modules.ownerOf(tokenId)).to.equal(owner.address);
      expect(await marketplace.listingCount()).to.equal(0);
    });
  });

  describe("operator loadouts and marketplace transfer", function () {
    it("enforces ownership and one module per slot", async function () {
      const { modules, minter, owner, other } = await deployFixture();
      const first = await mint(modules, minter, owner.address);
      const second = await mint(modules, minter, owner.address, radioSpec({
        moduleId: ethers.id("cabalmesh.radio.second.v1"),
        displayName: "Second Radio",
      }));

      await expect(modules.connect(other).equip(first))
        .to.be.revertedWithCustomError(modules, "NotTokenOwner")
        .withArgs(first);
      await expect(modules.connect(owner).equip(first))
        .to.emit(modules, "ModuleEquipped")
        .withArgs(first, owner.address, Slot.Radio);
      await expect(modules.connect(owner).equip(first))
        .to.be.revertedWithCustomError(modules, "TokenAlreadyEquipped")
        .withArgs(first, owner.address);
      await expect(modules.connect(owner).equip(second))
        .to.be.revertedWithCustomError(modules, "SlotAlreadyOccupied")
        .withArgs(owner.address, Slot.Radio, first);

      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(first);
      expect(await modules.equippedBy(first)).to.equal(owner.address);
    });

    it("allows distinct slots and explicit unequip", async function () {
      const { modules, minter, owner } = await deployFixture();
      const radio = await mint(modules, minter, owner.address);
      const crypto = await mint(modules, minter, owner.address, cryptoSpec());
      await modules.connect(owner).equip(radio);
      await modules.connect(owner).equip(crypto);

      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(radio);
      expect(await modules.equippedToken(owner.address, Slot.Crypto)).to.equal(crypto);

      await expect(modules.connect(owner).unequip(radio))
        .to.emit(modules, "ModuleUnequipped")
        .withArgs(radio, owner.address, Slot.Radio);
      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(0);
      expect(await modules.equippedBy(radio)).to.equal(ethers.ZeroAddress);
    });

    it("auto-unequips on transfer so one token cannot boost two operators", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      await modules.connect(owner).equip(tokenId);

      await expect(modules.connect(owner).transferFrom(owner.address, buyer.address, tokenId))
        .to.emit(modules, "ModuleUnequipped")
        .withArgs(tokenId, owner.address, Slot.Radio);

      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(0);
      expect(await modules.equippedBy(tokenId)).to.equal(ethers.ZeroAddress);
      await modules.connect(buyer).equip(tokenId);
      expect(await modules.equippedToken(buyer.address, Slot.Radio)).to.equal(tokenId);
    });

    it("clears the loadout if an eventual burn path destroys the token", async function () {
      const [admin, minter, owner] = await ethers.getSigners();
      const Harness = await ethers.getContractFactory("CabalMeshModulesHarness");
      const modules = await Harness.deploy(admin.address, minter.address, admin.address);
      await modules.waitForDeployment();
      const tokenId = await mint(modules, minter, owner.address);
      await modules.connect(owner).equip(tokenId);

      await expect(modules.burnForTest(tokenId))
        .to.emit(modules, "ModuleUnequipped")
        .withArgs(tokenId, owner.address, Slot.Radio);

      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(0);
      expect(await modules.equippedBy(tokenId)).to.equal(ethers.ZeroAddress);
      await expect(modules.ownerOf(tokenId))
        .to.be.revertedWithCustomError(modules, "ERC721NonexistentToken")
        .withArgs(tokenId);
    });

    it("stays equipped while merely listed, then clears on atomic escrow transfer", async function () {
      const { modules, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      const Marketplace = await ethers.getContractFactory("Marketplace");
      const marketplace = await Marketplace.deploy(await modules.getAddress(), 3 * 24 * 60 * 60);
      await marketplace.waitForDeployment();

      await modules.connect(owner).equip(tokenId);
      await modules.connect(owner).approve(await marketplace.getAddress(), tokenId);
      await marketplace.connect(owner).createListing("Relay Amplifier", 100, tokenId);
      expect(await modules.equippedBy(tokenId)).to.equal(owner.address);

      await marketplace.connect(buyer).buy(1, { value: 100 });
      expect(await modules.ownerOf(tokenId)).to.equal(await marketplace.getAddress());
      expect(await modules.equippedBy(tokenId)).to.equal(ethers.ZeroAddress);
      expect(await modules.equippedToken(owner.address, Slot.Radio)).to.equal(0);

      await marketplace.connect(buyer).releaseDeal(1);
      expect(await modules.ownerOf(tokenId)).to.equal(buyer.address);
      await modules.connect(buyer).equip(tokenId);
      expect(await modules.equippedBy(tokenId)).to.equal(buyer.address);
    });

    it("supports the verified unequip, list, cancel, and re-equip lifecycle", async function () {
      const { modules, minter, owner } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      const Marketplace = await ethers.getContractFactory("Marketplace");
      const marketplace = await Marketplace.deploy(await modules.getAddress(), 3 * 24 * 60 * 60);
      await marketplace.waitForDeployment();

      await modules.connect(owner).equip(tokenId);
      await expect(modules.connect(owner).unequip(tokenId))
        .to.emit(modules, "ModuleUnequipped")
        .withArgs(tokenId, owner.address, Slot.Radio);
      expect(await modules.equippedBy(tokenId)).to.equal(ethers.ZeroAddress);

      await modules.connect(owner).approve(await marketplace.getAddress(), tokenId);
      await expect(
        marketplace.connect(owner).createListingFor(
          await modules.getAddress(),
          "Canonical CabalMesh module",
          ethers.parseEther("2.40"),
          tokenId,
        ),
      ).to.emit(marketplace, "ListingCreated");
      expect(await marketplace.activeListingOf(await modules.getAddress(), tokenId)).to.equal(1n);
      expect(await modules.ownerOf(tokenId)).to.equal(owner.address);

      await expect(marketplace.connect(owner).cancelListing(1n))
        .to.emit(marketplace, "ListingCancelled")
        .withArgs(1n, owner.address, tokenId);
      expect(await marketplace.activeListingOf(await modules.getAddress(), tokenId)).to.equal(0n);
      await expect(modules.connect(owner).equip(tokenId))
        .to.emit(modules, "ModuleEquipped")
        .withArgs(tokenId, owner.address, Slot.Radio);
    });

    it("does not strand an already-funded deal if a module is revoked in escrow", async function () {
      const { modules, admin, minter, owner, buyer } = await deployFixture();
      const tokenId = await mint(modules, minter, owner.address);
      const Marketplace = await ethers.getContractFactory("Marketplace");
      const marketplace = await Marketplace.deploy(await modules.getAddress(), 3 * 24 * 60 * 60);
      await marketplace.waitForDeployment();
      await modules.connect(owner).approve(await marketplace.getAddress(), tokenId);
      await marketplace.connect(owner).createListing("Relay Amplifier", 100, tokenId);
      await marketplace.connect(buyer).buy(1, { value: 100 });

      await modules.connect(admin).revoke(tokenId, ethers.id("post-purchase issuer incident"));
      await expect(marketplace.connect(buyer).releaseDeal(1)).to.emit(
        marketplace,
        "DealReleased"
      );

      expect(await modules.ownerOf(tokenId)).to.equal(buyer.address);
      expect(await modules.revoked(tokenId)).to.equal(true);
      await expect(modules.connect(buyer).equip(tokenId))
        .to.be.revertedWithCustomError(modules, "NotLoadoutModule")
        .withArgs(tokenId);
    });
  });
});
