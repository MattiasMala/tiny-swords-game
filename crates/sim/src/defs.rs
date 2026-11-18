//! Definition tables and constants. OWNER: foundation.
//!
//! Source: docs/spec/01-defs-assets.md (§1-§7) plus the constant lists of 02 §1.2, 03a §1, 03b §1, 04a §1, 04b §1-§4
//! and 07 §2.1. Every number is verbatim from the spec; every Italian string is verbatim.
//!
//! Enum orders are load-bearing (network ids, bitmasks, job indices): `UnitKind`, `BuildingKind`, `TechId`, `Res`,
//! `KingdomColor` follow spec 01 §1 exactly; `#[repr(u8)]` discriminant = JS index.
//!
//! Render data: the client needs anchors / footprints / image sizes / crops / bar heights, so they are kept here
//! (pure data, no I/O). **Sprite file paths are NOT here**: the client builds them from the folder rules of spec 01
//! §2.5 / §3 / §8 (`Units/{Color} Units/{dir}/...`, `Extra/Units/{Color}/...`, `Buildings/{Color} Buildings/...`,
//! `Extra/Buildings/{Color}/...`, `Extra/Props/{name}.png`) using the `dir`, `extra`, `file`/`name()` fields below.

use crate::math::V2;
use serde::{Deserialize, Serialize};

macro_rules! indexed_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[repr(u8)]
        pub enum $name { $($(#[$vm])* $v),* }
        impl $name {
            /// All variants in declaration (= network id) order.
            pub const ALL: &'static [$name] = &[$($name::$v),*];
            pub const COUNT: usize = [$($name::$v),*].len();
            #[inline]
            pub const fn index(self) -> usize {
                self as usize
            }
            #[inline]
            pub fn from_index(i: usize) -> Option<$name> {
                Self::ALL.get(i).copied()
            }
        }
    };
}

// ------------------------------------------------------------------------------------------------------------------
// §1 Global enums
// ------------------------------------------------------------------------------------------------------------------

indexed_enum!(
    /// `UNIT_TYPES` (spec 01 §1): pawn, scout, warrior, lancer, archer, knight, monk, ram, catapult.
    UnitKind {
        Pawn,
        Scout,
        Warrior,
        Lancer,
        Archer,
        Knight,
        Monk,
        Ram,
        Catapult
    }
);

indexed_enum!(
    /// `BUILD_KINDS` (spec 01 §1).
    BuildingKind {
        Castle,
        House,
        Camp,
        Depot,
        Farm,
        Barracks,
        Wall,
        Archery,
        Stable,
        Smith,
        Market,
        Tower,
        Monastery,
        Siege
    }
);

indexed_enum!(
    /// `TECH_IDS` (spec 01 §1); bit `1 << index` in [`TechSet`].
    TechId {
        Age2,
        Age3,
        Carts,
        Masonry,
        Axes,
        Picks,
        Breeding,
        Boots,
        Horseshoes,
        Blades,
        Shields,
        Fletching,
        Armor,
        Maps,
        Herbs,
        Scriptures,
        Engineering,
    }
);

indexed_enum!(
    /// `RES_KINDS` = wood, gold, meat, stone (job index order, spec 01 §1).
    Res {
        Wood,
        Gold,
        Meat,
        Stone
    }
);

indexed_enum!(
    /// Capture point kinds (`POINT_DEF` key order, spec 01 §4).
    PointKind {
        Outpost,
        Mine,
        Quarry,
        Sawmill,
        Ranch
    }
);

indexed_enum!(
    /// `COLORS` = Blue, Red, Yellow, Purple, Black (order = `colorIdx`, spec 01 §1).
    KingdomColor {
        Blue,
        Red,
        Yellow,
        Purple,
        Black
    }
);

indexed_enum!(
    /// `AI_LEVELS` = easy, normal, hard (spec 04b §1). Default `Normal`.
    AiLevel { Easy, Normal, Hard }
);

indexed_enum!(
    /// Kind of a static resource node (spec 03b §2.1). Sheep are separate entities.
    NodeKind { Tree, Gold, Stone }
);

indexed_enum!(
    /// `PAWN_TOOLS` (spec 01 §1): Axe, Pickaxe, Knife, Hammer.
    PawnTool {
        Axe,
        Pickaxe,
        Knife,
        Hammer
    }
);

indexed_enum!(
    /// `LANCE_DIRS` / `dir5` (spec 01 §1, §2.5): Up, UpRight, Right, DownRight, Down. Horizontal facing = `flip`.
    Dir5 {
        Up,
        UpRight,
        Right,
        DownRight,
        Down
    }
);

indexed_enum!(
    /// Replicated animation state of a unit (JS anim names, spec 01 §2.5 / 03a §2.1).
    /// The client composes the sheet: `Attack`/`Block` + [`Dir5`] for lancers; `Attack` = `attack1`, `Attack2` =
    /// `attack2` for warriors; pawns: `Idle`/`Run` + carried load (`Wood|Gold|Meat|Stone`) else tool, `Work` + tool.
    AnimKind {
        /// JS `idle*`
        Idle,
        /// JS `run*`
        Run,
        /// JS `attack`, `attack1` (warrior), `attack<Dir5>` (lancer), `Heal` sheet for monks
        Attack,
        /// JS `attack2` (warrior combo)
        Attack2,
        /// JS `block`, `block<Dir5>` (lancer)
        Block,
        /// JS `work<Tool>` (pawn)
        Work,
    }
);

indexed_enum!(
    /// `ACT_LABEL` index (`u.act`, entities.js:34-35).
    Act {
        Idle,
        Marching,
        Fighting,
        ChopWood,
        MineGold,
        Butcher,
        Build,
        Repair,
        Carry,
        Heal,
        Guard,
        MineStone,
        Sheltered,
        Fleeing,
        Exploring,
    }
);

indexed_enum!(
    /// Particle FX keys (spec 01 §8.2).
    FxKind {
        Dust1,
        Dust2,
        Boom1,
        Boom2,
        Fire1,
        Fire2,
        Fire3,
        Splash,
        Heal
    }
);

indexed_enum!(
    /// Synthesized sound names (spec 07 §10.2).
    Sound {
        Hit,
        Block,
        Shoot,
        Chop,
        Coin,
        Heal,
        Build,
        Death,
        Boom,
        Click,
        Select,
        Ack,
        Research,
        Age,
        Deny,
        Horn,
        Win,
        Lose
    }
);

indexed_enum!(
    /// `Extra/Props/*.png` static props (spec 01 §8.3).
    PropKind {
        Barrel,
        Crate,
        Logs,
        Hay,
        Well,
        Signpost,
        Campfire,
        Cart,
        Tent,
        Mushrooms,
        Flowers,
        Statue,
        Column,
        Bones,
        Deadtree,
        Fence,
        Scarecrow,
        Grass,
        Chest,
        ChestOpen,
        Grave,
        Boat,
        Lilypads,
        Reeds,
        RuinArch,
        RuinWall,
        Menhir,
        StoneCircle,
        Boulder,
        Shrine,
        Obelisk,
        Windmill,
        Shipwreck,
        Dock,
        WagonWreck,
        Beehive,
        FlowersRed,
        FlowersBlue,
        FlowersWhite,
        TallGrass,
        Clover,
        Pebbles,
        Puddle,
        FallenLog,
        Stump,
        Pumpkins,
        Ferns,
        Sticks,
    }
);

/// Faction index = position in `World.factions` = base index (0..5). `side = idx + 1` on the map layers.
pub type FactionIdx = u8;

#[inline]
pub const fn side_of(f: FactionIdx) -> u8 {
    f + 1
}

// ------------------------------------------------------------------------------------------------------------------
// Strings (spec 01 §1)
// ------------------------------------------------------------------------------------------------------------------

/// `AGE_NAME` (index = age 1..3).
pub const AGE_NAME: [&str; 4] = ["", "Età del Villaggio", "Età Feudale", "Età dei Castelli"];
/// `AGE_SHORT` (crown numeral).
pub const AGE_SHORT: [&str; 4] = ["", "I", "II", "III"];
/// `FONT` css stack (client).
pub const FONT: &str = "Fredoka, \"Trebuchet MS\", Verdana, \"DejaVu Sans\", sans-serif";

/// `ACT_LABEL` (entities.js:34-35), index = [`Act`].
pub const ACT_LABEL: [&str; 15] = [
    "Inattivo",
    "In marcia",
    "In combattimento",
    "Taglia legna",
    "Estrae oro",
    "Macella pecore",
    "Costruisce",
    "Ripara",
    "Porta il carico",
    "Cura i feriti",
    "Di guardia",
    "Estrae pietra",
    "Al riparo",
    "In fuga",
    "In esplorazione",
];

impl Act {
    pub fn label(self) -> &'static str {
        ACT_LABEL[self as usize]
    }
}

impl Res {
    /// `RES_LABEL`.
    pub const fn label(self) -> &'static str {
        match self {
            Res::Wood => "Legna",
            Res::Gold => "Oro",
            Res::Meat => "Cibo",
            Res::Stone => "Pietra",
        }
    }
    /// `LOAD_NAME` / `PAWN_LOADS` sprite suffix.
    pub const fn load_name(self) -> &'static str {
        match self {
            Res::Wood => "Wood",
            Res::Gold => "Gold",
            Res::Meat => "Meat",
            Res::Stone => "Stone",
        }
    }
    /// `JOB_TOOL` (spec 01 §6).
    pub const fn tool(self) -> PawnTool {
        match self {
            Res::Wood => PawnTool::Axe,
            Res::Gold | Res::Stone => PawnTool::Pickaxe,
            Res::Meat => PawnTool::Knife,
        }
    }
    /// `JOB_TIME` seconds of work per load.
    pub const fn job_time(self) -> f32 {
        match self {
            Res::Wood => 3.0,
            Res::Gold | Res::Stone => 3.4,
            Res::Meat => 20.0,
        }
    }
    /// `RES_ACT` (panels.js:28).
    pub const fn act(self) -> Act {
        match self {
            Res::Wood => Act::ChopWood,
            Res::Gold => Act::MineGold,
            Res::Meat => Act::Butcher,
            Res::Stone => Act::MineStone,
        }
    }
    /// Market-tradable (wood, meat, stone; gold is the currency).
    pub const fn tradable(self) -> bool {
        !matches!(self, Res::Gold)
    }
}

impl PawnTool {
    pub const fn name(self) -> &'static str {
        match self {
            PawnTool::Axe => "Axe",
            PawnTool::Pickaxe => "Pickaxe",
            PawnTool::Knife => "Knife",
            PawnTool::Hammer => "Hammer",
        }
    }
}

impl Dir5 {
    pub const fn name(self) -> &'static str {
        match self {
            Dir5::Up => "Up",
            Dir5::UpRight => "UpRight",
            Dir5::Right => "Right",
            Dir5::DownRight => "DownRight",
            Dir5::Down => "Down",
        }
    }
}

/// `dir5(dx, dy)` (entities.js:48).
pub fn dir5(dx: f32, dy: f32) -> Dir5 {
    let a = (-dy).atan2(dx.abs());
    if a > 1.18 {
        Dir5::Up
    } else if a > 0.39 {
        Dir5::UpRight
    } else if a > -0.39 {
        Dir5::Right
    } else if a > -1.18 {
        Dir5::DownRight
    } else {
        Dir5::Down
    }
}

impl KingdomColor {
    /// English key used in asset paths (`Blue`...).
    pub const fn key(self) -> &'static str {
        match self {
            KingdomColor::Blue => "Blue",
            KingdomColor::Red => "Red",
            KingdomColor::Yellow => "Yellow",
            KingdomColor::Purple => "Purple",
            KingdomColor::Black => "Black",
        }
    }
    /// `COLOR_NAME`.
    pub const fn name_it(self) -> &'static str {
        match self {
            KingdomColor::Blue => "Blu",
            KingdomColor::Red => "Rosso",
            KingdomColor::Yellow => "Giallo",
            KingdomColor::Purple => "Viola",
            KingdomColor::Black => "Nero",
        }
    }
    /// `COLOR_CSS`.
    pub const fn css(self) -> &'static str {
        match self {
            KingdomColor::Blue => "#46a5bb",
            KingdomColor::Red => "#e6645c",
            KingdomColor::Yellow => "#e3c84e",
            KingdomColor::Purple => "#b981c6",
            KingdomColor::Black => "#7887a0",
        }
    }
    /// `COLOR_CSS` as RGB bytes.
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            KingdomColor::Blue => [0x46, 0xa5, 0xbb],
            KingdomColor::Red => [0xe6, 0x64, 0x5c],
            KingdomColor::Yellow => [0xe3, 0xc8, 0x4e],
            KingdomColor::Purple => [0xb9, 0x81, 0xc6],
            KingdomColor::Black => [0x78, 0x87, 0xa0],
        }
    }
    /// Kingdom name `'Regno ' + COLOR_NAME[color]` (factions.js:43), e.g. `Regno Blu`.
    pub fn kingdom_name(self) -> String {
        format!("Regno {}", self.name_it())
    }
}

impl AiLevel {
    pub const fn def(self) -> &'static AiLevelDef {
        &AI_LEVELS[self as usize]
    }
    pub const fn label(self) -> &'static str {
        self.def().label
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Costs, stocks, tech sets
// ------------------------------------------------------------------------------------------------------------------

/// Resource cost (integers in all tables).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    pub wood: u16,
    pub gold: u16,
    pub meat: u16,
    pub stone: u16,
}

impl Cost {
    pub const NONE: Cost = Cost {
        wood: 0,
        gold: 0,
        meat: 0,
        stone: 0,
    };

    pub const fn new(wood: u16, gold: u16, meat: u16, stone: u16) -> Cost {
        Cost {
            wood,
            gold,
            meat,
            stone,
        }
    }
    pub const fn get(&self, r: Res) -> u16 {
        match r {
            Res::Wood => self.wood,
            Res::Gold => self.gold,
            Res::Meat => self.meat,
            Res::Stone => self.stone,
        }
    }
    /// `(res, amount)` pairs in `RES_KINDS` order, zero entries skipped.
    pub fn iter(&self) -> impl Iterator<Item = (Res, u16)> + '_ {
        Res::ALL
            .iter()
            .map(|r| (*r, self.get(*r)))
            .filter(|(_, v)| *v > 0)
    }
    /// Sum of all amounts (`aiCost`, factions.js:542).
    pub const fn total(&self) -> u32 {
        self.wood as u32 + self.gold as u32 + self.meat as u32 + self.stone as u32
    }
    pub const fn is_free(&self) -> bool {
        self.total() == 0
    }
}

/// Faction resources (`f.res`): `f64`, fractional (spec 04a addenda E1), indexed by [`Res`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stock(pub [f64; 4]);

impl Stock {
    /// `START_RES = { wood: 200, gold: 100, meat: 200, stone: 100 }` (factions.js:10).
    pub const START: Stock = Stock([200.0, 100.0, 200.0, 100.0]);

    /// `canAfford(f, cost)`.
    pub fn can_afford(&self, c: &Cost) -> bool {
        Res::ALL
            .iter()
            .all(|r| self.0[*r as usize] >= c.get(*r) as f64)
    }
    /// `pay(f, cost)` (no check).
    pub fn pay(&mut self, c: &Cost) {
        for r in Res::ALL {
            self.0[*r as usize] -= c.get(*r) as f64;
        }
    }
    /// `refund(f, cost)`.
    pub fn refund(&mut self, c: &Cost) {
        for r in Res::ALL {
            self.0[*r as usize] += c.get(*r) as f64;
        }
    }
    /// `floor` of every amount (what snapshots/HUD show).
    pub fn floored(&self) -> [u32; 4] {
        self.0.map(|v| v.max(0.0).floor() as u32)
    }
}

impl core::ops::Index<Res> for Stock {
    type Output = f64;
    fn index(&self, r: Res) -> &f64 {
        &self.0[r as usize]
    }
}
impl core::ops::IndexMut<Res> for Stock {
    fn index_mut(&mut self, r: Res) -> &mut f64 {
        &mut self.0[r as usize]
    }
}

/// Set of techs as a bitmask `1 << TechId` (`bits(tech)` in net.js).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TechSet(pub u32);

impl TechSet {
    #[inline]
    pub const fn has(self, t: TechId) -> bool {
        self.0 & (1 << t as u32) != 0
    }
    #[inline]
    pub fn insert(&mut self, t: TechId) {
        self.0 |= 1 << t as u32;
    }
    #[inline]
    pub fn remove(&mut self, t: TechId) {
        self.0 &= !(1 << t as u32);
    }
    pub fn iter(self) -> impl Iterator<Item = TechId> {
        TechId::ALL.iter().copied().filter(move |t| self.has(*t))
    }
}

// ------------------------------------------------------------------------------------------------------------------
// §2 Units — UNIT_DEF
// ------------------------------------------------------------------------------------------------------------------

/// [SIM!] attack animation timing (spec 01 §2.5): hit at `floor(animT*fps) >= hit_frame`, action ends at `>= frames`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackAnim {
    pub frames: u8,
    pub fps: f32,
    pub hit_frame: u8,
}

impl AttackAnim {
    /// Action length in seconds (`frames / fps`).
    pub fn length(&self) -> f32 {
        self.frames as f32 / self.fps
    }
    /// Time of the hit (`hit_frame / fps`).
    pub fn hit_time(&self) -> f32 {
        self.hit_frame as f32 / self.fps
    }
}

/// Playback fps of the non-attack animations (presentation; spec 03a §2.1). `0` = frozen on frame 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimFps {
    pub idle: f32,
    pub run: f32,
    pub block: f32,
    pub work: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitDef {
    pub kind: UnitKind,
    /// JS key (`pawn`...).
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub hp: f32,
    /// px/s
    pub speed: f32,
    /// cells
    pub sight: f32,
    pub cost: Cost,
    /// training seconds
    pub time: f32,
    pub from: BuildingKind,
    pub age: u8,
    /// damage per hit; 0 = no `dmg` (pawn, scout, monk)
    pub dmg: f32,
    /// attack or heal reach, px
    pub range: f32,
    /// cooldown s (set at attack start)
    pub cd: f32,
    /// attack/heal animation timing; `None` for pawn and scout
    pub attack: Option<AttackAnim>,
    /// block probability (warrior, lancer)
    pub block: f32,
    /// auto-target radius px
    pub aggro: f32,
    /// damage multipliers vs unit kinds
    pub bonus: &'static [(UnitKind, f32)],
    /// damage multiplier vs buildings (`vsBuilding || 1`)
    pub vs_building: f32,
    pub foot: bool,
    pub siege: bool,
    pub only_buildings: bool,
    /// splash radius px (catapult)
    pub splash: f32,
    pub scout: bool,
    /// HP per heal (monk)
    pub heal: f32,
    pub faith: f32,
    pub faith_regen: f32,
    // ---- render data ----
    /// HP-bar height above the foot (px); also the click hit box (control.js:131)
    pub bar_y: f32,
    /// sprite folder in the pack (`Units/{C} Units/{dir}`), `None` = sheets in `Extra/Units/{C}`
    pub dir: Option<&'static str>,
    /// frame width / anchor of the idle sheet
    pub fw: u16,
    pub ax: u16,
    pub ay: u16,
    /// index in the avatar block (`Human Avatars`), `None` = portrait from the idle sheet
    pub avatar: Option<u8>,
    /// big selection ellipse (38x13: siege & knight) vs 24x10
    pub big_ring: bool,
    pub fps: AnimFps,
}

impl UnitDef {
    /// Combatant: `def.dmg` truthy (warrior, lancer, archer, knight, ram, catapult).
    pub fn has_dmg(&self) -> bool {
        self.dmg > 0.0
    }
    /// Soldier = threat to units: `dmg && !onlyBuildings` (also `aiFighter`).
    pub fn is_soldier(&self) -> bool {
        self.dmg > 0.0 && !self.only_buildings
    }
    /// Ranged: `range > 200` (archer, catapult).
    pub fn is_ranged(&self) -> bool {
        self.range > 200.0
    }
    /// Monk detection (`def.heal`).
    pub fn is_healer(&self) -> bool {
        self.heal > 0.0
    }
    /// `damageTo` multiplier vs a unit kind (`bonus[t] || 1`).
    pub fn bonus_vs(&self, t: UnitKind) -> f32 {
        self.bonus
            .iter()
            .find(|(k, _)| *k == t)
            .map(|(_, m)| *m)
            .unwrap_or(1.0)
    }
    /// Formation line (`lineOf`, entities.js:346): 0 front, 1 archers, 2 catapults, 3 rear (no dmg).
    pub fn line(&self) -> u8 {
        if self.dmg <= 0.0 {
            3
        } else if self.splash > 0.0 {
            2
        } else if self.range > 200.0 {
            1
        } else {
            0
        }
    }
}

const fn fps(idle: f32, run: f32, block: f32, work: f32) -> AnimFps {
    AnimFps {
        idle,
        run,
        block,
        work,
    }
}

const fn atk(frames: u8, fps: f32, hit_frame: u8) -> Option<AttackAnim> {
    Some(AttackAnim {
        frames,
        fps,
        hit_frame,
    })
}

/// `UNIT_DEF` (assets.js:112-169), index = [`UnitKind`].
pub const UNIT_DEFS: [UnitDef; 9] = [
    UnitDef {
        kind: UnitKind::Pawn,
        key: "pawn",
        label: "Pedone",
        desc: "Raccoglie risorse, costruisce e ripara.",
        hp: 50.0,
        speed: 92.0,
        sight: 5.0,
        cost: Cost::new(0, 0, 50, 0),
        time: 13.0,
        from: BuildingKind::Castle,
        age: 1,
        dmg: 0.0,
        range: 0.0,
        cd: 0.0,
        attack: None,
        block: 0.0,
        aggro: 0.0,
        bonus: &[],
        vs_building: 1.0,
        foot: false,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 78.0,
        dir: Some("Pawn"),
        fw: 192,
        ax: 96,
        ay: 134,
        avatar: Some(4),
        big_ring: false,
        fps: fps(8.0, 10.0, 0.0, 10.0),
    },
    UnitDef {
        kind: UnitKind::Scout,
        key: "scout",
        label: "Esploratore",
        desc: "Veloce e con la vista lunga: esplora la mappa, anche da solo (tasto E), e occupa i punti liberi. Non combatte.",
        hp: 60.0,
        speed: 165.0,
        sight: 10.0,
        cost: Cost::new(0, 0, 40, 0),
        time: 12.0,
        from: BuildingKind::Castle,
        age: 1,
        dmg: 0.0,
        range: 0.0,
        cd: 0.0,
        attack: None,
        block: 0.0,
        aggro: 0.0,
        bonus: &[],
        vs_building: 1.0,
        foot: false,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: true,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 78.0,
        dir: None,
        fw: 96,
        ax: 48,
        ay: 86,
        avatar: None,
        big_ring: false,
        fps: fps(8.0, 10.0, 0.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Warrior,
        key: "warrior",
        label: "Guerriero",
        desc: "Fante con scudo, disponibile da subito. Forte contro Lancieri ed edifici, teme gli Arcieri.",
        hp: 130.0,
        speed: 98.0,
        sight: 6.0,
        cost: Cost::new(0, 40, 58, 0),
        time: 15.0,
        from: BuildingKind::Barracks,
        age: 1,
        dmg: 13.0,
        range: 52.0,
        cd: 0.95,
        attack: atk(4, 12.0, 2),
        block: 0.25,
        aggro: 300.0,
        bonus: &[(UnitKind::Lancer, 1.5)],
        vs_building: 1.6,
        foot: true,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 92.0,
        dir: Some("Warrior"),
        fw: 192,
        ax: 96,
        ay: 134,
        avatar: Some(0),
        big_ring: false,
        fps: fps(8.0, 10.0, 10.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Lancer,
        key: "lancer",
        label: "Lanciere",
        desc: "Colpisce da lontano con la lancia. Forte contro i Cavalieri, teme i Guerrieri.",
        hp: 110.0,
        speed: 118.0,
        sight: 6.0,
        cost: Cost::new(0, 40, 60, 0),
        time: 17.0,
        from: BuildingKind::Barracks,
        age: 2,
        dmg: 20.0,
        range: 88.0,
        cd: 1.25,
        attack: atk(3, 9.0, 1),
        block: 0.2,
        aggro: 320.0,
        bonus: &[
            (UnitKind::Knight, 1.5),
            (UnitKind::Archer, 1.1),
            (UnitKind::Monk, 1.5),
        ],
        vs_building: 1.0,
        foot: true,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 92.0,
        dir: Some("Lancer"),
        fw: 320,
        ax: 160,
        ay: 196,
        avatar: Some(1),
        big_ring: false,
        fps: fps(10.0, 10.0, 10.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Archer,
        key: "archer",
        label: "Arciere",
        desc: "Tira da lontano. Forte contro i Guerrieri, fragile in mischia: teme i Cavalieri.",
        hp: 70.0,
        speed: 102.0,
        sight: 7.0,
        cost: Cost::new(38, 32, 0, 0),
        time: 17.0,
        from: BuildingKind::Archery,
        age: 2,
        dmg: 12.0,
        range: 330.0,
        cd: 1.5,
        attack: atk(8, 14.0, 5),
        block: 0.0,
        aggro: 390.0,
        bonus: &[(UnitKind::Warrior, 1.7)],
        vs_building: 0.5,
        foot: true,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 92.0,
        dir: Some("Archer"),
        fw: 192,
        ax: 96,
        ay: 134,
        avatar: Some(2),
        big_ring: false,
        fps: fps(8.0, 10.0, 0.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Knight,
        key: "knight",
        label: "Cavaliere",
        desc: "Cavalleria veloce e resistente. Travolge Arcieri e Catapulte, teme i Lancieri.",
        hp: 190.0,
        speed: 150.0,
        sight: 7.0,
        cost: Cost::new(0, 60, 80, 0),
        time: 20.0,
        from: BuildingKind::Stable,
        age: 2,
        dmg: 18.0,
        range: 58.0,
        cd: 1.1,
        attack: atk(6, 12.0, 3),
        block: 0.0,
        aggro: 340.0,
        bonus: &[
            (UnitKind::Archer, 1.6),
            (UnitKind::Catapult, 2.0),
            (UnitKind::Pawn, 1.3),
        ],
        vs_building: 1.0,
        foot: false,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 106.0,
        dir: None,
        fw: 128,
        ax: 64,
        ay: 112,
        avatar: None,
        big_ring: true,
        fps: fps(8.0, 12.0, 0.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Monk,
        key: "monk",
        label: "Monaco",
        desc: "Cura le unità alleate ferite. Ha una riserva di 100 punti di cura che si ricarica piano. I nemici vicini lo preferiscono ad altri bersagli.",
        hp: 65.0,
        speed: 94.0,
        sight: 6.0,
        cost: Cost::new(0, 80, 0, 0),
        time: 22.0,
        from: BuildingKind::Monastery,
        age: 3,
        dmg: 0.0,
        range: 170.0,
        cd: 1.8,
        attack: atk(11, 12.0, 6),
        block: 0.0,
        aggro: 0.0,
        bonus: &[],
        vs_building: 1.0,
        foot: false,
        siege: false,
        only_buildings: false,
        splash: 0.0,
        scout: false,
        heal: 40.0,
        faith: 100.0,
        faith_regen: 0.4,
        bar_y: 78.0,
        dir: Some("Monk"),
        fw: 192,
        ax: 96,
        ay: 134,
        avatar: Some(3),
        big_ring: false,
        fps: fps(8.0, 10.0, 0.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Ram,
        key: "ram",
        label: "Ariete",
        desc: "Sfonda edifici e mura. Non può colpire le unità; le frecce gli fanno poco.",
        hp: 340.0,
        speed: 62.0,
        sight: 5.0,
        cost: Cost::new(160, 60, 0, 0),
        time: 26.0,
        from: BuildingKind::Siege,
        age: 3,
        dmg: 55.0,
        range: 60.0,
        cd: 2.2,
        attack: atk(4, 7.0, 2),
        block: 0.0,
        aggro: 330.0,
        bonus: &[],
        vs_building: 4.0,
        foot: false,
        siege: true,
        only_buildings: true,
        splash: 0.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 100.0,
        dir: None,
        fw: 128,
        ax: 64,
        ay: 117,
        avatar: None,
        big_ring: true,
        // addenda A1: run animates at 8 fps; idle = frame 0 of run
        fps: fps(0.0, 8.0, 0.0, 0.0),
    },
    UnitDef {
        kind: UnitKind::Catapult,
        key: "catapult",
        label: "Catapulta",
        desc: "Lancia massi a grande distanza: colpiscono tutti i nemici in una zona.",
        hp: 110.0,
        speed: 58.0,
        sight: 8.0,
        cost: Cost::new(140, 120, 0, 0),
        time: 28.0,
        from: BuildingKind::Siege,
        age: 3,
        dmg: 42.0,
        range: 430.0,
        cd: 4.2,
        attack: atk(4, 7.0, 2),
        block: 0.0,
        aggro: 450.0,
        bonus: &[],
        vs_building: 2.2,
        foot: false,
        siege: true,
        only_buildings: false,
        splash: 72.0,
        scout: false,
        heal: 0.0,
        faith: 0.0,
        faith_regen: 0.0,
        bar_y: 74.0,
        dir: None,
        fw: 96,
        ax: 48,
        ay: 85,
        avatar: None,
        big_ring: true,
        fps: fps(0.0, 8.0, 0.0, 0.0),
    },
];

impl UnitKind {
    #[inline]
    pub const fn def(self) -> &'static UnitDef {
        &UNIT_DEFS[self as usize]
    }
}

// ------------------------------------------------------------------------------------------------------------------
// §3 Buildings — BUILD_DEF
// ------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingDef {
    pub kind: BuildingKind,
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    /// footprint in cells
    pub w: i32,
    pub h: i32,
    pub hp: f32,
    pub cost: Cost,
    /// construction seconds for one pawn
    pub work: f32,
    /// cells
    pub sight: f32,
    /// minimum era (castle: 1, never placeable)
    pub age: u8,
    pub pawns: u16,
    pub troops: u16,
    /// resources that can be delivered here
    pub drop: &'static [Res],
    pub trains: &'static [UnitKind],
    pub techs: &'static [TechId],
    /// tower/wall: placeable only near castle (12) or own outpost (9)
    pub defense: bool,
    /// market
    pub trade: bool,
    /// sprite not team-coloured (wall)
    pub neutral: bool,
    /// number of sprite variants (houses: 3)
    pub variants: u8,
    // ---- render data ----
    /// sprite lives in `Extra/Buildings` (else `Buildings/{C} Buildings`)
    pub extra: bool,
    /// sprite file stem (`Castle`, `House1`... — houses append the variant number 1..3)
    pub file: &'static str,
    pub img_w: u16,
    pub img_h: u16,
    pub ax: u16,
    pub ay: u16,
    /// visible sprite rect `[sx, sy, sw, sh]` (thumbnails)
    pub crop: [u16; 4],
}

impl BuildingDef {
    /// Accepts deliveries of `r`.
    pub fn accepts(&self, r: Res) -> bool {
        self.drop.contains(&r)
    }
    /// Arrow muzzle height above the foot `def.ay * 0.62` (tower 143.84, castle 155; spec 03b §9).
    pub fn muzzle(&self) -> f32 {
        self.ay as f32 * 0.62
    }
}

/// `BUILD_DEF` (assets.js:217-305), index = [`BuildingKind`].
pub const BUILDING_DEFS: [BuildingDef; 14] = [
    BuildingDef {
        kind: BuildingKind::Castle,
        key: "castle",
        label: "Castello",
        desc: "Cuore del regno: se cade, hai perso. Addestra Pedoni ed Esploratori e fa avanzare d'era.",
        w: 5,
        h: 2,
        hp: 2600.0,
        cost: Cost::NONE,
        work: 0.0,
        sight: 10.0,
        age: 1,
        pawns: 10,
        troops: 6,
        drop: &[Res::Wood, Res::Gold, Res::Meat, Res::Stone],
        trains: &[UnitKind::Pawn, UnitKind::Scout],
        techs: &[TechId::Age2, TechId::Age3, TechId::Carts, TechId::Masonry],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: false,
        file: "Castle",
        img_w: 320,
        img_h: 256,
        ax: 160,
        ay: 250,
        crop: [4, 41, 312, 208],
    },
    BuildingDef {
        kind: BuildingKind::House,
        key: "house",
        label: "Casa",
        desc: "+5 Pedoni al massimo. Con i nemici in vista i Pedoni ci si rifugiano.",
        w: 2,
        h: 1,
        hp: 320.0,
        cost: Cost::new(30, 0, 0, 0),
        work: 9.0,
        sight: 4.0,
        age: 1,
        pawns: 5,
        troops: 0,
        drop: &[],
        trains: &[],
        techs: &[],
        defense: false,
        trade: false,
        neutral: false,
        variants: 3,
        extra: false,
        file: "House",
        img_w: 128,
        img_h: 192,
        ax: 64,
        ay: 178,
        crop: [8, 16, 112, 157],
    },
    BuildingDef {
        kind: BuildingKind::Camp,
        key: "camp",
        label: "Accampamento",
        desc: "+8 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 420.0,
        cost: Cost::new(60, 0, 0, 0),
        work: 12.0,
        sight: 5.0,
        age: 1,
        pawns: 0,
        troops: 8,
        drop: &[],
        trains: &[],
        techs: &[],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Camp",
        img_w: 200,
        img_h: 189,
        ax: 100,
        ay: 179,
        crop: [6, 6, 188, 177],
    },
    BuildingDef {
        kind: BuildingKind::Depot,
        key: "depot",
        label: "Deposito",
        desc: "Punto di consegna per legna, oro e pietra. Costruiscilo vicino a boschi e cave.",
        w: 2,
        h: 1,
        hp: 380.0,
        cost: Cost::new(60, 0, 0, 0),
        work: 12.0,
        sight: 5.0,
        age: 1,
        pawns: 0,
        troops: 0,
        drop: &[Res::Wood, Res::Gold, Res::Stone],
        trains: &[],
        techs: &[TechId::Axes, TechId::Picks],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Depot",
        img_w: 136,
        img_h: 118,
        ax: 68,
        ay: 108,
        crop: [6, 6, 124, 106],
    },
    BuildingDef {
        kind: BuildingKind::Farm,
        key: "farm",
        label: "Fattoria",
        desc: "Alleva pecore nei dintorni, abbastanza per due o tre pedoni, e fa da punto di consegna per il cibo.",
        w: 3,
        h: 2,
        hp: 420.0,
        cost: Cost::new(150, 0, 0, 0),
        work: 14.0,
        sight: 5.0,
        age: 1,
        pawns: 0,
        troops: 0,
        drop: &[Res::Meat],
        trains: &[],
        techs: &[TechId::Breeding],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Farm",
        img_w: 202,
        img_h: 167,
        ax: 101,
        ay: 157,
        crop: [6, 6, 190, 155],
    },
    BuildingDef {
        kind: BuildingKind::Barracks,
        key: "barracks",
        label: "Caserma",
        desc: "Addestra Guerrieri e, dall'Età Feudale, Lancieri. +2 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 700.0,
        cost: Cost::new(90, 0, 0, 0),
        work: 18.0,
        sight: 5.0,
        age: 1,
        pawns: 0,
        troops: 2,
        drop: &[],
        trains: &[UnitKind::Warrior, UnitKind::Lancer],
        techs: &[TechId::Boots],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: false,
        file: "Barracks",
        img_w: 192,
        img_h: 256,
        ax: 96,
        ay: 246,
        crop: [4, 58, 184, 187],
    },
    BuildingDef {
        kind: BuildingKind::Wall,
        key: "wall",
        label: "Palizzata",
        desc: "Sbarra il passaggio ai nemici; le tue unità ci passano. Solo vicino al castello o a un tuo avamposto. Maiusc + trascina per posarne una fila.",
        w: 1,
        h: 1,
        hp: 300.0,
        cost: Cost::new(12, 0, 0, 0),
        work: 3.0,
        sight: 2.0,
        age: 1,
        pawns: 0,
        troops: 0,
        drop: &[],
        trains: &[],
        techs: &[],
        defense: true,
        trade: false,
        neutral: true,
        variants: 1,
        extra: true,
        file: "Wall",
        img_w: 76,
        img_h: 78,
        ax: 38,
        ay: 68,
        crop: [6, 6, 64, 66],
    },
    BuildingDef {
        kind: BuildingKind::Archery,
        key: "archery",
        label: "Arcieria",
        desc: "Addestra Arcieri. +2 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 600.0,
        cost: Cost::new(80, 30, 0, 0),
        work: 18.0,
        sight: 5.0,
        age: 2,
        pawns: 0,
        troops: 2,
        drop: &[],
        trains: &[UnitKind::Archer],
        techs: &[],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: false,
        file: "Archery",
        img_w: 192,
        img_h: 256,
        ax: 96,
        ay: 242,
        crop: [3, 61, 183, 179],
    },
    BuildingDef {
        kind: BuildingKind::Stable,
        key: "stable",
        label: "Scuderia",
        desc: "Addestra Cavalieri. +2 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 650.0,
        cost: Cost::new(110, 40, 0, 0),
        work: 18.0,
        sight: 5.0,
        age: 2,
        pawns: 0,
        troops: 2,
        drop: &[],
        trains: &[UnitKind::Knight],
        techs: &[TechId::Horseshoes],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Stable",
        img_w: 200,
        img_h: 174,
        ax: 100,
        ay: 164,
        crop: [6, 6, 188, 162],
    },
    BuildingDef {
        kind: BuildingKind::Smith,
        key: "smith",
        label: "Fucina",
        desc: "Migliora armi e armature dell'esercito.",
        w: 3,
        h: 2,
        hp: 650.0,
        cost: Cost::new(100, 0, 0, 40),
        work: 18.0,
        sight: 5.0,
        age: 2,
        pawns: 0,
        troops: 0,
        drop: &[],
        trains: &[],
        techs: &[
            TechId::Blades,
            TechId::Shields,
            TechId::Fletching,
            TechId::Armor,
        ],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Smith",
        img_w: 196,
        img_h: 184,
        ax: 98,
        ay: 174,
        crop: [6, 6, 184, 172],
    },
    BuildingDef {
        kind: BuildingKind::Market,
        key: "market",
        label: "Mercato",
        desc: "Scambia legna, cibo e pietra con oro.",
        w: 3,
        h: 2,
        hp: 600.0,
        cost: Cost::new(120, 0, 0, 0),
        work: 18.0,
        sight: 5.0,
        age: 2,
        pawns: 0,
        troops: 0,
        drop: &[],
        trains: &[],
        techs: &[TechId::Maps],
        defense: false,
        trade: true,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Market",
        img_w: 202,
        img_h: 207,
        ax: 101,
        ay: 197,
        crop: [6, 6, 190, 195],
    },
    BuildingDef {
        kind: BuildingKind::Tower,
        key: "tower",
        label: "Torre",
        desc: "Scaglia frecce sui nemici vicini e vede lontano. Solo vicino al castello o a un tuo avamposto.",
        w: 2,
        h: 1,
        hp: 620.0,
        cost: Cost::new(40, 0, 0, 80),
        work: 16.0,
        sight: 9.0,
        age: 2,
        pawns: 0,
        troops: 0,
        drop: &[],
        trains: &[],
        techs: &[],
        defense: true,
        trade: false,
        neutral: false,
        variants: 1,
        extra: false,
        file: "Tower",
        img_w: 128,
        img_h: 256,
        ax: 64,
        ay: 232,
        crop: [4, 46, 120, 184],
    },
    BuildingDef {
        kind: BuildingKind::Monastery,
        key: "monastery",
        label: "Monastero",
        desc: "Addestra Monaci guaritori. +2 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 600.0,
        cost: Cost::new(80, 100, 0, 40),
        work: 22.0,
        sight: 5.0,
        age: 3,
        pawns: 0,
        troops: 2,
        drop: &[],
        trains: &[UnitKind::Monk],
        techs: &[TechId::Herbs, TechId::Scriptures],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: false,
        file: "Monastery",
        img_w: 192,
        img_h: 320,
        ax: 96,
        ay: 312,
        crop: [16, 45, 160, 265],
    },
    BuildingDef {
        kind: BuildingKind::Siege,
        key: "siege",
        label: "Officina d'assedio",
        desc: "Costruisce Arieti e Catapulte. +2 truppe al massimo.",
        w: 3,
        h: 2,
        hp: 650.0,
        cost: Cost::new(160, 60, 0, 0),
        work: 22.0,
        sight: 5.0,
        age: 3,
        pawns: 0,
        troops: 2,
        drop: &[],
        trains: &[UnitKind::Ram, UnitKind::Catapult],
        techs: &[TechId::Engineering],
        defense: false,
        trade: false,
        neutral: false,
        variants: 1,
        extra: true,
        file: "Siege",
        img_w: 202,
        img_h: 192,
        ax: 101,
        ay: 182,
        crop: [6, 6, 190, 180],
    },
];

impl BuildingKind {
    #[inline]
    pub const fn def(self) -> &'static BuildingDef {
        &BUILDING_DEFS[self as usize]
    }
    /// In `BUILD_MENU` (placeable by command; castle never).
    pub fn placeable(self) -> bool {
        BUILD_MENU.contains(&self)
    }
}

/// `BUILD_MENU` (build-panel order; whitelist for the `place` command).
pub const BUILD_MENU: [BuildingKind; 13] = [
    BuildingKind::House,
    BuildingKind::Camp,
    BuildingKind::Depot,
    BuildingKind::Farm,
    BuildingKind::Barracks,
    BuildingKind::Wall,
    BuildingKind::Archery,
    BuildingKind::Stable,
    BuildingKind::Smith,
    BuildingKind::Market,
    BuildingKind::Tower,
    BuildingKind::Monastery,
    BuildingKind::Siege,
];

/// Research-screen row order `TREE_ROWS` (panels.js).
pub const TREE_ROWS: [BuildingKind; 9] = [
    BuildingKind::Castle,
    BuildingKind::Depot,
    BuildingKind::Farm,
    BuildingKind::Barracks,
    BuildingKind::Stable,
    BuildingKind::Smith,
    BuildingKind::Market,
    BuildingKind::Monastery,
    BuildingKind::Siege,
];

/// Tower / castle shooting (spec 03b §9): range (+`mods.range`), damage (+`mods.atk[archer]`), reload.
pub const TOWER_RANGE: f32 = 420.0;
pub const CASTLE_RANGE: f32 = 380.0;
pub const TOWER_DMG: f32 = 15.0;
pub const CASTLE_DMG: f32 = 14.0;
pub const TOWER_RELOAD: f32 = 1.1;
pub const CASTLE_RELOAD: f32 = 1.2;
/// no target → rescan after 0.3 s
pub const SHOOT_RESCAN: f32 = 0.3;

/// Wall stake slicing (render, spec 01 §3.2).
pub const STAKE_SX: u16 = 31;
pub const STAKE_SW: u16 = 14;
pub const STAKE_N: u8 = 5;

// ------------------------------------------------------------------------------------------------------------------
// §4 Capture points — POINT_DEF
// ------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointDef {
    pub kind: PointKind,
    pub key: &'static str,
    pub label: &'static str,
    /// `Extra/Buildings/{sprite}.png` (neutral)
    pub sprite: &'static str,
    /// income resource (0.5/s), `None` for the outpost
    pub res: Option<Res>,
    /// outpost: allows building within 9 cells
    pub zone: bool,
    pub img_w: u16,
    pub img_h: u16,
    pub ax: u16,
    pub ay: u16,
    pub crop: [u16; 4],
    pub desc: &'static str,
}

pub const POINT_DEFS: [PointDef; 5] = [
    PointDef {
        kind: PointKind::Outpost,
        key: "outpost",
        label: "Avamposto",
        sprite: "Outpost",
        res: None,
        zone: true,
        img_w: 168,
        img_h: 230,
        ax: 84,
        ay: 220,
        crop: [6, 6, 156, 218],
        desc: "Chi lo tiene può costruire nei dintorni, torri e palizzate comprese.",
    },
    PointDef {
        kind: PointKind::Mine,
        key: "mine",
        label: "Miniera",
        sprite: "Mine",
        res: Some(Res::Gold),
        zone: false,
        img_w: 168,
        img_h: 150,
        ax: 84,
        ay: 140,
        crop: [6, 6, 156, 138],
        desc: "Rende oro a chi la tiene, senza Pedoni.",
    },
    PointDef {
        kind: PointKind::Quarry,
        key: "quarry",
        label: "Cava",
        sprite: "Quarry",
        res: Some(Res::Stone),
        zone: false,
        img_w: 168,
        img_h: 161,
        ax: 84,
        ay: 151,
        crop: [6, 6, 156, 149],
        desc: "Rende pietra a chi la tiene, senza Pedoni.",
    },
    PointDef {
        kind: PointKind::Sawmill,
        key: "sawmill",
        label: "Segheria",
        sprite: "Sawmill",
        res: Some(Res::Wood),
        zone: false,
        img_w: 168,
        img_h: 162,
        ax: 84,
        ay: 152,
        crop: [6, 6, 156, 150],
        desc: "Rende legna a chi la tiene, senza Pedoni.",
    },
    PointDef {
        kind: PointKind::Ranch,
        key: "ranch",
        label: "Ovile",
        sprite: "Ranch",
        res: Some(Res::Meat),
        zone: false,
        img_w: 168,
        img_h: 145,
        ax: 84,
        ay: 135,
        crop: [6, 6, 156, 133],
        desc: "Rende cibo a chi lo tiene, senza Pedoni.",
    },
];

impl PointKind {
    #[inline]
    pub const fn def(self) -> &'static PointDef {
        &POINT_DEFS[self as usize]
    }
}

/// `POINT_INCOME` res/s (= 30/min).
pub const POINT_INCOME: f32 = 0.5;
/// `CAPTURE_TIME` s.
pub const CAPTURE_TIME: f32 = 8.0;
/// `CAPTURE_RADIUS` px (circle centred 64 px above the anchor; also `|dx| <= 300`).
pub const CAPTURE_RADIUS: f32 = 300.0;
/// `POINT_SIGHT` cells (sight centre `(p.x, p.y - 64)`).
pub const POINT_SIGHT: f32 = 7.0;
/// Capture check period (s).
pub const CAPTURE_TICK: f32 = 0.5;

// ------------------------------------------------------------------------------------------------------------------
// §5 Research — TECH_DEF
// ------------------------------------------------------------------------------------------------------------------

/// Icon spec (`drawSpec`, spec 01 §9.6): `Age(n)` = crown + numeral, `X(name)` = `Extra/Icons/{name}.png`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechIcon {
    Age(u8),
    X(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TechDef {
    pub id: TechId,
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    /// building that researches it
    pub at: BuildingKind,
    /// minimum era (also the tech-tree column)
    pub age: u8,
    pub req: Option<TechId>,
    pub cost: Cost,
    /// seconds (never accelerated)
    pub time: f32,
    pub icon: TechIcon,
}

pub const TECH_DEFS: [TechDef; 17] = [
    TechDef {
        id: TechId::Age2,
        key: "age2",
        label: "Età Feudale",
        desc: "Sblocca Arcieria, Scuderia, Fucina, Mercato, Torre, Lancieri e nuove ricerche.",
        at: BuildingKind::Castle,
        age: 1,
        req: None,
        cost: Cost::new(0, 100, 250, 0),
        time: 40.0,
        icon: TechIcon::Age(2),
    },
    TechDef {
        id: TechId::Age3,
        key: "age3",
        label: "Età dei Castelli",
        desc: "Sblocca Monastero, Officina d'assedio e le ricerche avanzate.",
        at: BuildingKind::Castle,
        age: 2,
        req: Some(TechId::Age2),
        cost: Cost::new(0, 300, 400, 100),
        time: 55.0,
        icon: TechIcon::Age(3),
    },
    TechDef {
        id: TechId::Carts,
        key: "carts",
        label: "Carriole",
        desc: "I Pedoni portano 5 risorse in più e camminano più svelti.",
        at: BuildingKind::Castle,
        age: 2,
        req: None,
        cost: Cost::new(120, 0, 80, 0),
        time: 28.0,
        icon: TechIcon::X("barrow"),
    },
    TechDef {
        id: TechId::Masonry,
        key: "masonry",
        label: "Muratura",
        desc: "+30% di resistenza a tutti gli edifici.",
        at: BuildingKind::Castle,
        age: 3,
        req: None,
        cost: Cost::new(100, 0, 0, 150),
        time: 36.0,
        icon: TechIcon::X("tower"),
    },
    TechDef {
        id: TechId::Axes,
        key: "axes",
        label: "Asce affilate",
        desc: "I Pedoni tagliano la legna il 30% più in fretta.",
        at: BuildingKind::Depot,
        age: 1,
        req: None,
        cost: Cost::new(60, 0, 80, 0),
        time: 24.0,
        icon: TechIcon::X("axe"),
    },
    TechDef {
        id: TechId::Picks,
        key: "picks",
        label: "Picconi d'acciaio",
        desc: "I Pedoni estraggono oro e pietra il 30% più in fretta.",
        at: BuildingKind::Depot,
        age: 2,
        req: Some(TechId::Axes),
        cost: Cost::new(60, 0, 80, 0),
        time: 24.0,
        icon: TechIcon::X("pickaxe"),
    },
    TechDef {
        id: TechId::Breeding,
        key: "breeding",
        label: "Allevamento",
        desc: "Le Fattorie allevano il 50% più in fretta.",
        at: BuildingKind::Farm,
        age: 1,
        req: None,
        cost: Cost::new(80, 40, 0, 0),
        time: 24.0,
        icon: TechIcon::X("wheat"),
    },
    TechDef {
        id: TechId::Boots,
        key: "boots",
        label: "Marcia forzata",
        desc: "Guerrieri, Lancieri e Arcieri si muovono il 12% più veloci.",
        at: BuildingKind::Barracks,
        age: 2,
        req: None,
        cost: Cost::new(0, 40, 100, 0),
        time: 26.0,
        icon: TechIcon::X("boot"),
    },
    TechDef {
        id: TechId::Horseshoes,
        key: "horseshoes",
        label: "Ferratura",
        desc: "I Cavalieri sono il 15% più veloci.",
        at: BuildingKind::Stable,
        age: 2,
        req: None,
        cost: Cost::new(0, 80, 100, 0),
        time: 28.0,
        icon: TechIcon::X("horseshoe"),
    },
    TechDef {
        id: TechId::Blades,
        key: "blades",
        label: "Lame temprate",
        desc: "+4 attacco a Guerrieri, Lancieri e Cavalieri.",
        at: BuildingKind::Smith,
        age: 2,
        req: None,
        cost: Cost::new(0, 80, 100, 0),
        time: 30.0,
        icon: TechIcon::X("sword"),
    },
    TechDef {
        id: TechId::Shields,
        key: "shields",
        label: "Scudi rinforzati",
        desc: "Guerrieri, Lancieri e Cavalieri subiscono metà del danno dalle frecce.",
        at: BuildingKind::Smith,
        age: 2,
        req: None,
        cost: Cost::new(120, 60, 0, 0),
        time: 30.0,
        icon: TechIcon::X("shield"),
    },
    TechDef {
        id: TechId::Fletching,
        key: "fletching",
        label: "Impennaggio",
        desc: "+3 attacco e più gittata ad Arcieri, Torri e Castello.",
        at: BuildingKind::Smith,
        age: 2,
        req: None,
        cost: Cost::new(100, 60, 0, 0),
        time: 30.0,
        icon: TechIcon::X("arrows"),
    },
    TechDef {
        id: TechId::Armor,
        key: "armor",
        label: "Corazze",
        desc: "Tutti i soldati subiscono 3 danni in meno per colpo.",
        at: BuildingKind::Smith,
        age: 3,
        req: Some(TechId::Shields),
        cost: Cost::new(0, 120, 120, 0),
        time: 36.0,
        icon: TechIcon::X("breastplate"),
    },
    TechDef {
        id: TechId::Maps,
        key: "maps",
        label: "Cartografia",
        desc: "Unità ed edifici vedono 2 caselle più lontano.",
        at: BuildingKind::Market,
        age: 2,
        req: None,
        cost: Cost::new(80, 60, 0, 0),
        time: 24.0,
        icon: TechIcon::X("spyglass"),
    },
    TechDef {
        id: TechId::Herbs,
        key: "herbs",
        label: "Erbe curative",
        desc: "I Monaci curano il 50% in più e ricaricano la riserva il 50% più in fretta.",
        at: BuildingKind::Monastery,
        age: 3,
        req: None,
        cost: Cost::new(0, 120, 0, 0),
        time: 30.0,
        icon: TechIcon::X("herbs"),
    },
    TechDef {
        id: TechId::Scriptures,
        key: "scriptures",
        label: "Sacre scritture",
        desc: "I Monaci curano da più lontano e resistono il doppio.",
        at: BuildingKind::Monastery,
        age: 3,
        req: Some(TechId::Herbs),
        cost: Cost::new(60, 100, 0, 0),
        time: 30.0,
        icon: TechIcon::X("book"),
    },
    TechDef {
        id: TechId::Engineering,
        key: "engineering",
        label: "Ingegneria",
        desc: "+25% di danno ad Arieti e Catapulte.",
        at: BuildingKind::Siege,
        age: 3,
        req: None,
        cost: Cost::new(150, 120, 0, 0),
        time: 36.0,
        icon: TechIcon::X("anvil"),
    },
];

impl TechId {
    #[inline]
    pub const fn def(self) -> &'static TechDef {
        &TECH_DEFS[self as usize]
    }
    /// Age techs set `f.age` instead of mods.
    pub const fn age_target(self) -> Option<u8> {
        match self {
            TechId::Age2 => Some(2),
            TechId::Age3 => Some(3),
            _ => None,
        }
    }
}

/// Faction modifiers `f.mods` (factions.js:46-50). Effects are assignments (each tech only once).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mods {
    /// flat attack bonus per unit kind (only warrior/lancer/archer/knight are ever set)
    pub atk: [f32; 9],
    /// archer range + tower/castle range (fletching 45)
    pub range: f32,
    /// armour of warrior/lancer/archer/knight (armor 3)
    pub armor: f32,
    /// arrow multiplier on warrior/lancer/knight (shields 0.5)
    pub arrow_res: f32,
    /// ram/catapult damage (engineering 1.25)
    pub siege: f32,
    /// gather speed per resource (axes wood 1.3; picks gold+stone 1.3)
    pub gather: [f32; 4],
    /// extra carry (carts 5)
    pub carry: f32,
    pub pawn_speed: f32,
    /// warrior/lancer/archer speed (boots 1.12)
    pub foot: f32,
    /// knight speed (horseshoes 1.15)
    pub horse: f32,
    /// heal amount and faith regen (herbs 1.5)
    pub heal: f32,
    /// monk range + (scriptures 70)
    pub heal_range: f32,
    /// monk hp x (scriptures 2)
    pub monk_hp: f32,
    /// farm breeding speed (breeding 1.5)
    pub breed: f32,
    /// building max hp x (masonry 1.3)
    pub build_hp: f32,
    /// +cells of sight (maps 2)
    pub sight: f32,
}

impl Default for Mods {
    fn default() -> Self {
        Mods {
            atk: [0.0; 9],
            range: 0.0,
            armor: 0.0,
            arrow_res: 1.0,
            siege: 1.0,
            gather: [1.0; 4],
            carry: 0.0,
            pawn_speed: 1.0,
            foot: 1.0,
            horse: 1.0,
            heal: 1.0,
            heal_range: 0.0,
            monk_hp: 1.0,
            breed: 1.0,
            build_hp: 1.0,
            sight: 0.0,
        }
    }
}

impl Mods {
    /// `techMods(m, id)` (factions.js:81-99). Age techs change nothing here.
    pub fn apply_tech(&mut self, t: TechId) {
        match t {
            TechId::Age2 | TechId::Age3 => {}
            TechId::Carts => {
                self.carry = 5.0;
                self.pawn_speed = 1.15;
            }
            TechId::Masonry => self.build_hp = 1.3,
            TechId::Axes => self.gather[Res::Wood as usize] = 1.3,
            TechId::Picks => {
                self.gather[Res::Gold as usize] = 1.3;
                self.gather[Res::Stone as usize] = 1.3;
            }
            TechId::Breeding => self.breed = 1.5,
            TechId::Boots => self.foot = 1.12,
            TechId::Horseshoes => self.horse = 1.15,
            TechId::Blades => {
                self.atk[UnitKind::Warrior as usize] += 4.0;
                self.atk[UnitKind::Lancer as usize] += 4.0;
                self.atk[UnitKind::Knight as usize] += 4.0;
            }
            TechId::Shields => self.arrow_res = 0.5,
            TechId::Fletching => {
                self.atk[UnitKind::Archer as usize] += 3.0;
                self.range = 45.0;
            }
            TechId::Armor => self.armor = 3.0,
            TechId::Maps => self.sight = 2.0,
            TechId::Herbs => self.heal = 1.5,
            TechId::Scriptures => {
                self.heal_range = 70.0;
                self.monk_hp = 2.0;
            }
            TechId::Engineering => self.siege = 1.25,
        }
    }

    /// Mods of a faction that researched `techs` (clients rebuild mods from the bitmask).
    pub fn from_techs(techs: TechSet) -> Mods {
        let mut m = Mods::default();
        for t in techs.iter() {
            m.apply_tech(t);
        }
        m
    }

    /// `unitMaxHp(f, type)` = `round(hp * (monk ? monkHp : 1))`.
    pub fn unit_max_hp(&self, k: UnitKind) -> f32 {
        let d = k.def();
        (d.hp
            * if k == UnitKind::Monk {
                self.monk_hp
            } else {
                1.0
            })
        .round()
    }
    /// `unitSpeed(f, type)`.
    pub fn unit_speed(&self, k: UnitKind) -> f32 {
        let d = k.def();
        d.speed
            * if k == UnitKind::Pawn {
                self.pawn_speed
            } else if d.foot {
                self.foot
            } else if k == UnitKind::Knight {
                self.horse
            } else {
                1.0
            }
    }
    /// `unitDmg(u)` = `(dmg + atk[type]) * (siege ? mods.siege : 1)`.
    pub fn unit_dmg(&self, k: UnitKind) -> f32 {
        let d = k.def();
        (d.dmg + self.atk[k as usize]) * if d.siege { self.siege } else { 1.0 }
    }
    /// `unitRange(u)` = `range + (archer ? mods.range : monk ? mods.healRange : 0)`.
    pub fn unit_range(&self, k: UnitKind) -> f32 {
        k.def().range
            + match k {
                UnitKind::Archer => self.range,
                UnitKind::Monk => self.heal_range,
                _ => 0.0,
            }
    }
    /// `unitArmor(u)` = `dmg && !siege ? mods.armor : 0`.
    pub fn unit_armor(&self, k: UnitKind) -> f32 {
        let d = k.def();
        if d.dmg > 0.0 && !d.siege {
            self.armor
        } else {
            0.0
        }
    }
    /// Sight radius in px of a unit (`sightPx`).
    pub fn unit_sight_px(&self, k: UnitKind) -> f32 {
        (k.def().sight + self.sight) * crate::TILE
    }
    /// `round(def.hp * buildHp)`.
    pub fn building_max_hp(&self, k: BuildingKind) -> f32 {
        (k.def().hp * self.build_hp).round()
    }
    /// Load capacity `CARRY + carry`.
    pub fn carry_cap(&self) -> f32 {
        CARRY + self.carry
    }
}

/// `trainSpeed(n) = 1 + TRAIN_BONUS * clamp(n-1, 0, 5)` (n = finished buildings of that kind; techs ×1).
pub fn train_speed(n: u32) -> f32 {
    1.0 + TRAIN_BONUS * (n.max(1) - 1).min(5) as f32
}

/// `sellPrice(f, res) = max(20, 50 - trades*6)` (gold received for 100).
pub fn sell_price(trades: f64) -> f64 {
    (SELL_PRICE - trades * SELL_DROP).max(20.0)
}

/// `buyPrice(f, res) = min(320, 130 + trades*10)` (gold paid for 100).
pub fn buy_price(trades: f64) -> f64 {
    (BUY_PRICE + trades * BUY_RISE).min(320.0)
}

// ------------------------------------------------------------------------------------------------------------------
// §6 Simulation constants (entities.js:5-33, factions.js:4-10, world.js:6-21)
// ------------------------------------------------------------------------------------------------------------------

/// `NODE_USERS` (tree 2, gold 3, stone 3; sheep 1 = [`SHEEP_USERS`]).
pub const fn node_users(k: NodeKind) -> u8 {
    match k {
        NodeKind::Tree => 2,
        NodeKind::Gold | NodeKind::Stone => 3,
    }
}
pub const SHEEP_USERS: u8 = 1;
/// max pawns on a big (2x2) deposit
pub const BIG_USERS: u8 = 8;
/// `NODE_RES`
pub const fn node_res(k: NodeKind) -> Res {
    match k {
        NodeKind::Tree => Res::Wood,
        NodeKind::Gold => Res::Gold,
        NodeKind::Stone => Res::Stone,
    }
}
pub const CARRY: f32 = 10.0;
pub const SHEEP_MEAT: f32 = 15.0;
pub const TREE_WOOD: f32 = 40.0;
pub const GOLD_MAX: f32 = 240.0;
pub const STONE_MAX: f32 = 300.0;
pub const BIG_GOLD: f32 = 1500.0;
pub const BIG_STONE: f32 = 1800.0;
/// tree regrows `TREE_WOOD` after this many seconds at 0 wood
pub const TREE_REGROW: f32 = 110.0;
/// gold/stone veins regain `VEIN_REGEN_AMOUNT` every `VEIN_REGEN_TIME` s up to max (also when depleted)
pub const VEIN_REGEN_TIME: f32 = 25.0;
pub const VEIN_REGEN_AMOUNT: f32 = 10.0;
pub const FARM_BREED: f32 = 9.0;
pub const FARM_SHEEP: usize = 5;
pub const FARM_START_SHEEP: usize = 2;
pub const TREASURE: f32 = 50.0;
pub const BUILDING_ARMOR: f32 = 0.4;
pub const REPAIR_RATE: f32 = 0.035;
/// site starts at `maxHp * 0.1`; builders add `maxHp * 0.9 * dt / work`
pub const SITE_START_HP: f32 = 0.1;
pub const ALERT_EVERY: f32 = 12.0;
pub const ALARM_TIME: f32 = 10.0;
pub const HOME_RADIUS: f32 = 1500.0;
pub const RAID_RADIUS: f32 = 900.0;
pub const SHELTER_CAP: usize = 5;
pub const CALM_TIME: f32 = 4.0;
/// pawn threat-scan period (`fearT`)
pub const FEAR_PERIOD: f32 = 0.4;
pub const HEALER_FOCUS: f32 = 0.6;
pub const KITE_RANGE: f32 = 110.0;
/// kite only while `cd > KITE_MIN_CD`
pub const KITE_MIN_CD: f32 = 0.3;
pub const MONK_SPACE: f32 = 150.0;
/// monk enemy scan radius `150 * 1.6`
pub const MONK_FOE_SCAN: f32 = 240.0;
/// monk auto-heal scan radius / threshold
pub const MONK_HEAL_SCAN: f32 = 430.0;
pub const MONK_HEAL_BELOW: f32 = 0.9;
/// monk escort lead scan radius
pub const MONK_LEAD_SCAN: f32 = 1500.0;
pub const ESCORT_GAP: f32 = 200.0;
pub const LINE_GAP: f32 = 44.0;
/// extra gap between formation lines
pub const LINE_EXTRA: f32 = 36.0;
pub const TRAIN_BONUS: f32 = 0.2;
pub const PAWN_MAX: u16 = 30;
pub const TROOP_MAX: u16 = 50;
pub const QUEUE_MAX: usize = 5;
pub const RAID_SPACING: f32 = 120.0;
pub const WORK_RADIUS: f32 = 2600.0;
/// cells, for towers/walls from the castle centre
pub const DEFENSE_RADIUS: f32 = 12.0;
/// cells, other buildings from the castle centre
pub const BUILD_RADIUS: f32 = 40.0;
/// cells from an owned outpost (any kind)
pub const OUTPOST_RADIUS: f32 = 9.0;
/// cells explored around the castle at start
pub const START_SIGHT: f32 = 15.0;
/// under-construction buildings see 2 cells
pub const SITE_SIGHT: f32 = 2.0;
/// building sight centre is `(b.x, b.y - 30)`
pub const BUILDING_SIGHT_DY: f32 = 30.0;
/// "no room" toast throttle (s)
pub const HOUSED_TOAST_EVERY: f32 = 15.0;
/// market
pub const SELL_PRICE: f64 = 50.0;
pub const SELL_DROP: f64 = 6.0;
pub const BUY_PRICE: f64 = 130.0;
pub const BUY_RISE: f64 = 10.0;
pub const TRADE_AMOUNT: f64 = 100.0;
/// `trades[k]` recovery per 1-s faction tick
pub const TRADE_DECAY: f64 = 0.04;
/// test-mode gift (`TEST_GIFT`)
pub const TEST_GIFT: f64 = 1000.0;

// units / combat (spec 03a §1)
pub const SEP_CELL: f32 = 64.0;
pub const SEP_RADIUS: f32 = 32.0;
pub const SEP_STRENGTH: f32 = 0.12;
pub const SEP_BUSY_WEIGHT: f32 = 0.2;
pub const GUARD_LEASH: f32 = 430.0;
pub const AI_THREAT_RADIUS: f32 = 950.0;
pub const AI_THREAT_MEMORY: f32 = 5.0;
pub const BUILDING_AGGRO_MIN: f32 = 260.0;
pub const RAM_BUILDING_AGGRO: f32 = 520.0;
pub const BREACH_RADIUS: f32 = 900.0;
pub const BREACH_CANDIDATES: usize = 4;
pub const BREACH_LOCK: f32 = 8.0;
/// amove ends > 200 px from goal → breach
pub const AMOVE_BREACH_DIST: f32 = 200.0;
pub const ARROW_SPEED: f32 = 640.0;
pub const ARROW_OVERSHOOT: f32 = 70.0;
pub const ARROW_HIT_DX: f32 = 18.0;
pub const ARROW_HIT_DY: f32 = 26.0;
/// unit hit box centre is `y - 28`
pub const ARROW_HIT_YOFF: f32 = 28.0;
/// arrows hit buildings in `y0 - 70 < y < y1`
pub const ARROW_BUILDING_TOP: f32 = 70.0;
pub const ARROW_VS_RAM: f32 = 0.2;
/// archer arrow spawn `(x ± 16, y - 34)`
pub const ARCHER_MUZZLE: V2 = V2 { x: 16.0, y: 34.0 };
pub const BLOCK_MULT: f32 = 0.35;
pub const BLOCK_TIME: f32 = 0.5;
pub const HURT_FLASH: f32 = 0.12;
pub const ROCK_BASE_TIME: f32 = 0.5;
pub const ROCK_SPEED: f32 = 520.0;
pub const ROCK_ARC: f32 = 150.0;
/// rock spawn `(x ± 20, y - 50)`
pub const ROCK_MUZZLE: V2 = V2 { x: 20.0, y: 50.0 };
pub const ROCK_SPIN: f32 = 9.0;
pub const PAWN_TARGET_WEIGHT: f32 = 2.2;
pub const MELEE_CONFIRM: f32 = 1.5;
pub const MELEE_BUILDING_EXTRA: f32 = 44.0;
/// unit in range of a unit target: `d <= range + 6`
pub const RANGE_SLACK: f32 = 6.0;
pub const CLEARLINE_DIST: f32 = 280.0;
pub const STALL_NEAR: f32 = 120.0;
pub const STALL_SHORT: f32 = 1.2;
pub const STALL_LONG: f32 = 4.0;
pub const STALL_PROGRESS: f32 = 3.0;
/// default `travel` arrival radius
pub const TRAVEL_NEAR: f32 = 10.0;
/// `holdPost`: walk back if > 48 px, arrive within 24
pub const HOLD_POST_DIST: f32 = 48.0;
pub const HOLD_POST_NEAR: f32 = 24.0;
/// `tryMove` look-ahead probe (px)
pub const MOVE_PROBE: f32 = 9.0;
/// soldier think interval `rand(0.3, 0.6)`, monk `rand(0.4, 0.7)`
pub const SOLDIER_THINK: (f32, f32) = (0.3, 0.6);
pub const MONK_THINK: (f32, f32) = (0.4, 0.7);
/// recall point `(castle.x, castle.y1 + 130)`
pub const RECALL_DY: f32 = 130.0;
/// default rally `(b.x, b.y1 + 72)`, farm pasture `(b.x, b.y1 + 70)`, deploy spot `y1 + 24`
pub const RALLY_DY: f32 = 72.0;
pub const PASTURE_DY: f32 = 70.0;
pub const DEPLOY_DY: f32 = 24.0;
/// scout explore arrival
pub const EXPLORE_NEAR: f32 = 40.0;

// sheep / pickups (spec 03b §2)
pub const SHEEP_SPEED: f32 = 38.0;
pub const SHEEP_PER_MEADOW: usize = 3;
pub const SHEEP_RESPAWN: f32 = 10.0;
pub const SHEEP_FIRST_RESPAWN: f32 = 5.0;
pub const SHEEP_ARRIVE: f32 = 36.0;
pub const PICKUP_SCAN: f32 = 0.2;
pub const PICKUP_HOP: f32 = 0.4;
pub const LOOT_AMOUNT: f32 = 15.0;
pub const LOOT_LIFE: f32 = 60.0;
/// pickup grab box `|dx| <= 32, |dy| <= 28`
pub const PICKUP_DX: f32 = 32.0;
pub const PICKUP_DY: f32 = 28.0;
/// harvest/deliver pauses (s)
pub const PAUSE_HARVEST: f32 = 0.5;
pub const PAUSE_DELIVER: f32 = 0.4;
pub const PAUSE_NO_NODE: f32 = 1.0;
/// `chop` sound every 0.6 s of work
pub const CHOP_EVERY: f32 = 0.6;
/// node search radius around the order origin (px)
pub const NODE_SEARCH: f32 = 900.0;
/// `afterBuild`: next site within 700 px, auto node within 520 px
pub const NEXT_SITE_RADIUS: f32 = 700.0;
pub const AUTO_NODE_RADIUS: f32 = 520.0;

// fog (spec 05 §12, 04a §11.1)
/// sim exploration update period (s)
pub const EXPLORE_PERIOD: f32 = 0.25;
/// visible-set update period (5 Hz, ARCHITECTURE.md)
pub const VISIBLE_PERIOD: f32 = 0.2;
/// faction census period (s)
pub const CENSUS_PERIOD: f32 = 1.0;

// map / pathing (spec 02 §1.2)
pub const BASE_COUNT: usize = 5;
pub const BASE_GAP: f32 = 64.0;
/// half sizes of the obstacle-free zone around each base
pub const BASE_CLEAR: (i32, i32) = (9, 7);
/// land is guaranteed on `BASE_CLEAR + 3`
pub const BASE_LAND: (i32, i32) = (12, 10);
/// `BASE_STOCK`: per base, `[kind, radius cells, min count]` (spec 02 §5.2)
pub const BASE_STOCK: [(NodeKind, f32, usize); 6] = [
    (NodeKind::Tree, 20.0, 12),
    (NodeKind::Gold, 22.0, 4),
    (NodeKind::Stone, 22.0, 2),
    (NodeKind::Tree, 14.0, 6),
    (NodeKind::Gold, 14.0, 2),
    (NodeKind::Stone, 14.0, 1),
];
/// a meadow within this many cells of each base
pub const BASE_MEADOW: f32 = 14.0;
/// 8-neighbour order E, W, S, N, SE, SW, NE, NW (tie-break order of `stepFrom`)
pub const DIRS8: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (-1, 1),
    (1, -1),
    (-1, -1),
];
pub const FIELD_CACHE: usize = 128;
/// stale fields recomputed per tick (JS 2 per ~60 fps frame → 6 per 20 Hz tick, spec 02 addenda W5)
pub const FIELD_REFRESH: i32 = 6;
/// group members per tick that may switch to their own slot path (W5)
pub const FIELD_NEW: i32 = 6;
/// extra cost (tenths of a cell) per own-building cell in cross fields
pub const CROSS_COST: u32 = 30;
/// distance-field unreachable sentinel
pub const FIELD_INF: u32 = u32::MAX;
/// capture point kinds cycled by placement index
pub const POINT_MIX: [PointKind; 6] = [
    PointKind::Mine,
    PointKind::Sawmill,
    PointKind::Outpost,
    PointKind::Quarry,
    PointKind::Ranch,
    PointKind::Outpost,
];
/// capture points ≥ 20 cells from bases and ≥ 11 from other marks; big deposits 18 / 9
pub const POINT_BASE_GAP: f32 = 20.0;
pub const POINT_MARK_GAP: f32 = 11.0;
pub const BIG_BASE_GAP: f32 = 18.0;
pub const BIG_MARK_GAP: f32 = 9.0;
/// chests and scattered groups ≥ 14 cells from every base
pub const FAR_FROM_BASES: f32 = 14.0;

// day / night (spec 07 §2.1)
/// sim seconds per full day
pub const DAY_LENGTH: f32 = 480.0;
/// time 0 = phase 0.1 (morning)
pub const DAY_OFFSET: f32 = 0.1;
/// `[phase, ambient rgb, shadow strength]`
pub const DAY_KEYS: [(f32, [u8; 3], f32); 8] = [
    (0.00, [255, 200, 172], 0.20),
    (0.07, [255, 251, 240], 0.30),
    (0.50, [255, 251, 240], 0.30),
    (0.60, [255, 206, 150], 0.28),
    (0.67, [208, 142, 150], 0.20),
    (0.73, [108, 124, 196], 0.14),
    (0.95, [108, 124, 196], 0.14),
    (1.00, [255, 200, 172], 0.20),
];

/// Day phase in `[0, 1)` for a sim time (`(time / 480 + 0.1) % 1`).
pub fn day_phase(time: f32) -> f32 {
    (time / DAY_LENGTH + DAY_OFFSET).rem_euclid(1.0)
}

// ------------------------------------------------------------------------------------------------------------------
// AI (spec 04b §1-§4)
// ------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiLevelDef {
    pub level: AiLevel,
    /// `Facile` / `Normale` / `Difficile`
    pub label: &'static str,
    /// passive income per second, each resource
    pub trickle: f64,
    /// seconds between two thinks (also the dt passed to think)
    pub think: f32,
    /// desired pawns per age 1..3
    pub pawns: [u16; 3],
    pub army: u16,
    /// seconds before the first wave `[min, max]`
    pub first: (f32, f32),
    /// seconds between later waves
    pub gap: (f32, f32),
    pub farms: u16,
    pub factories: u16,
    pub trade: bool,
    pub counter: f32,
    pub garrison: f32,
    pub odds: f32,
    pub retreat: f32,
    pub reinforce: bool,
    pub rally: bool,
    pub recall: bool,
    pub towers: u16,
    pub monks: u16,
    pub siege: u16,
    pub capture: bool,
}

impl AiLevelDef {
    /// `waveNeed` cap = `round(army*(1-garrison)) - 1` (easy 7, normal 14, hard 24).
    pub fn wave_need_cap(&self) -> u16 {
        ((self.army as f32 * (1.0 - self.garrison)).round() as u16).saturating_sub(1)
    }
}

pub const AI_LEVELS: [AiLevelDef; 3] = [
    AiLevelDef {
        level: AiLevel::Easy,
        label: "Facile",
        trickle: 0.0,
        think: 2.4,
        pawns: [7, 10, 12],
        army: 9,
        first: (420.0, 540.0),
        gap: (160.0, 220.0),
        farms: 2,
        factories: 1,
        trade: false,
        counter: 0.3,
        garrison: 0.1,
        odds: 0.8,
        retreat: 0.2,
        reinforce: false,
        rally: false,
        recall: false,
        towers: 0,
        monks: 1,
        siege: 1,
        capture: false,
    },
    AiLevelDef {
        level: AiLevel::Normal,
        label: "Normale",
        trickle: 0.0,
        think: 1.5,
        pawns: [9, 13, 16],
        army: 18,
        first: (360.0, 450.0),
        gap: (160.0, 200.0),
        farms: 3,
        factories: 1,
        trade: false,
        counter: 1.0,
        garrison: 0.15,
        odds: 0.8,
        retreat: 0.3,
        reinforce: true,
        rally: false,
        recall: false,
        towers: 0,
        monks: 1,
        siege: 1,
        capture: true,
    },
    AiLevelDef {
        level: AiLevel::Hard,
        label: "Difficile",
        trickle: 0.6,
        think: 1.0,
        pawns: [12, 18, 22],
        army: 36,
        first: (240.0, 300.0),
        gap: (90.0, 120.0),
        farms: 4,
        factories: 2,
        trade: true,
        counter: 1.8,
        garrison: 0.3,
        odds: 1.1,
        retreat: 0.45,
        reinforce: true,
        rally: true,
        recall: true,
        towers: 3,
        monks: 3,
        siege: 2,
        capture: true,
    },
];

/// One `AI_PLAN` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStep {
    Build(BuildingKind),
    Tech(TechId),
}

/// `AI_PLAN` (factions.js:28-37), 29 entries.
pub const AI_PLAN: [PlanStep; 29] = {
    use BuildingKind as B;
    use PlanStep::{Build, Tech};
    use TechId as T;
    [
        Build(B::Farm),
        Build(B::Barracks),
        Build(B::Depot),
        Tech(T::Age2),
        Build(B::Farm),
        Build(B::Archery),
        Build(B::Smith),
        Build(B::Farm),
        Build(B::Stable),
        Tech(T::Blades),
        Build(B::Tower),
        Tech(T::Axes),
        Build(B::Market),
        Build(B::Farm),
        Tech(T::Age3),
        Build(B::Monastery),
        Tech(T::Armor),
        Build(B::Siege),
        Tech(T::Picks),
        Tech(T::Fletching),
        Build(B::Tower),
        Tech(T::Carts),
        Tech(T::Horseshoes),
        Tech(T::Herbs),
        Tech(T::Breeding),
        Tech(T::Boots),
        Tech(T::Masonry),
        Tech(T::Engineering),
        Tech(T::Scriptures),
    ]
};

/// `AI_BASE` army weights (warrior 3, lancer 2, archer 2, knight 2).
pub const AI_BASE: [(UnitKind, f32); 4] = [
    (UnitKind::Warrior, 3.0),
    (UnitKind::Lancer, 2.0),
    (UnitKind::Archer, 2.0),
    (UnitKind::Knight, 2.0),
];
/// "at home" radius around the castle (px)
pub const AI_HOME: f32 = 1400.0;
/// initial AI timers
pub const AI_THINK_START: (f32, f32) = (1.0, 3.0);
pub const AI_WAVE_NEED_START: u16 = 4;
pub const AI_SHUFFLE_START: f32 = 20.0;
pub const AI_CAPTURE_START: f32 = 60.0;

/// `aiFighter(type) = dmg && !onlyBuildings`.
pub fn ai_fighter(k: UnitKind) -> bool {
    k.def().is_soldier()
}

/// `aiCost(type)` = sum of the cost.
pub fn ai_cost(k: UnitKind) -> f32 {
    k.def().cost.total() as f32
}

// ------------------------------------------------------------------------------------------------------------------
// §8.3 Props, trees, resource sprites (render data)
// ------------------------------------------------------------------------------------------------------------------

impl PropKind {
    /// File stem in `Extra/Props/` (JS name).
    pub const fn name(self) -> &'static str {
        PROP_INFO[self as usize].0
    }
    /// Image size `[w, h]` (anchored bottom-centre).
    pub const fn size(self) -> (u16, u16) {
        let p = PROP_INFO[self as usize];
        (p.1, p.2)
    }
}

/// `EXTRA.props` names and sizes, index = [`PropKind`].
pub const PROP_INFO: [(&str, u16, u16); 48] = [
    ("barrel", 34, 40),
    ("crate", 39, 40),
    ("logs", 50, 40),
    ("hay", 51, 44),
    ("well", 60, 76),
    ("signpost", 45, 58),
    ("campfire", 49, 44),
    ("cart", 69, 46),
    ("tent", 80, 84),
    ("mushrooms", 44, 40),
    ("flowers", 56, 36),
    ("statue", 51, 84),
    ("column", 62, 60),
    ("bones", 51, 34),
    ("deadtree", 86, 88),
    ("fence", 65, 40),
    ("scarecrow", 58, 68),
    ("grass", 43, 36),
    ("chest", 45, 38),
    ("chest_open", 41, 44),
    ("grave", 55, 50),
    ("boat", 59, 44),
    ("lilypads", 42, 34),
    ("reeds", 49, 52),
    ("ruin_arch", 102, 96),
    ("ruin_wall", 95, 60),
    ("menhir", 63, 84),
    ("stone_circle", 73, 56),
    ("boulder", 58, 56),
    ("shrine", 64, 80),
    ("obelisk", 54, 92),
    ("windmill", 99, 124),
    ("shipwreck", 72, 64),
    ("dock", 64, 48),
    ("wagon_wreck", 70, 50),
    ("beehive", 38, 46),
    ("flowers_red", 51, 34),
    ("flowers_blue", 49, 34),
    ("flowers_white", 46, 32),
    ("tall_grass", 52, 40),
    ("clover", 39, 26),
    ("pebbles", 44, 26),
    ("puddle", 53, 30),
    ("fallen_log", 51, 36),
    ("stump", 49, 34),
    ("pumpkins", 56, 38),
    ("ferns", 55, 40),
    ("sticks", 41, 24),
];

/// Landmarks cycle of the JS generator (solid props, spec 02 §5.13).
pub const LANDMARKS: [PropKind; 28] = {
    use PropKind as P;
    [
        P::Well,
        P::Statue,
        P::Column,
        P::RuinArch,
        P::Deadtree,
        P::Boulder,
        P::Deadtree,
        P::Scarecrow,
        P::Grave,
        P::Menhir,
        P::Signpost,
        P::RuinWall,
        P::Fence,
        P::StoneCircle,
        P::Cart,
        P::Hay,
        P::Logs,
        P::Barrel,
        P::Shrine,
        P::Column,
        P::Obelisk,
        P::Grave,
        P::WagonWreck,
        P::Beehive,
        P::Signpost,
        P::Fence,
        P::Windmill,
        P::Boulder,
    ]
};

/// `TREE_DEF[v]` = `(ay, stumpAy)` for tree sprite variants 0..3 (fw 192, ax 96, 8 fps, 8 frames).
pub const TREE_DEF: [(u16, u16); 4] = [(240, 238), (248, 244), (170, 231), (168, 227)];
/// tree entity foot = cell centre + 22 px south
pub const TREE_FOOT_DY: f32 = 22.0;

/// FX sheet data `(file, fw, ax, ay, fps, frames)` (spec 01 §8.2); `Heal` file is per colour
/// (`Units/{C} Units/Monk/Heal_Effect.png`).
pub const FX_INFO: [(&str, u16, u16, u16, f32, u8); 9] = [
    ("Particle FX/Dust_01.png", 64, 32, 46, 14.0, 8),
    ("Particle FX/Dust_02.png", 64, 32, 56, 14.0, 10),
    ("Particle FX/Explosion_01.png", 192, 96, 110, 14.0, 8),
    ("Particle FX/Explosion_02.png", 192, 96, 130, 14.0, 10),
    ("Particle FX/Fire_01.png", 64, 32, 62, 10.0, 8),
    ("Particle FX/Fire_02.png", 64, 32, 62, 10.0, 10),
    ("Particle FX/Fire_03.png", 64, 32, 62, 10.0, 12),
    ("Particle FX/Water Splash.png", 192, 96, 100, 12.0, 9),
    ("Heal_Effect.png", 192, 96, 134, 12.0, 11),
];

impl Sound {
    /// JS sound name.
    pub const fn name(self) -> &'static str {
        match self {
            Sound::Hit => "hit",
            Sound::Block => "block",
            Sound::Shoot => "shoot",
            Sound::Chop => "chop",
            Sound::Coin => "coin",
            Sound::Heal => "heal",
            Sound::Build => "build",
            Sound::Death => "death",
            Sound::Boom => "boom",
            Sound::Click => "click",
            Sound::Select => "select",
            Sound::Ack => "ack",
            Sound::Research => "research",
            Sound::Age => "age",
            Sound::Deny => "deny",
            Sound::Horn => "horn",
            Sound::Win => "win",
            Sound::Lose => "lose",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_indexed_by_enum() {
        for (i, d) in UNIT_DEFS.iter().enumerate() {
            assert_eq!(d.kind as usize, i);
        }
        for (i, d) in BUILDING_DEFS.iter().enumerate() {
            assert_eq!(d.kind as usize, i);
        }
        for (i, d) in TECH_DEFS.iter().enumerate() {
            assert_eq!(d.id as usize, i);
        }
        for (i, d) in POINT_DEFS.iter().enumerate() {
            assert_eq!(d.kind as usize, i);
        }
        for (i, d) in AI_LEVELS.iter().enumerate() {
            assert_eq!(d.level as usize, i);
        }
        assert_eq!(PROP_INFO.len(), PropKind::COUNT);
        assert_eq!(FX_INFO.len(), FxKind::COUNT);
        assert_eq!(UnitKind::COUNT, 9);
        assert_eq!(BuildingKind::COUNT, 14);
        assert_eq!(TechId::COUNT, 17);
    }

    #[test]
    fn derived_stats() {
        let mut m = Mods::default();
        assert_eq!(m.unit_max_hp(UnitKind::Monk), 65.0);
        m.apply_tech(TechId::Scriptures);
        assert_eq!(m.unit_max_hp(UnitKind::Monk), 130.0);
        m.apply_tech(TechId::Blades);
        assert_eq!(m.unit_dmg(UnitKind::Warrior), 17.0);
        m.apply_tech(TechId::Engineering);
        assert_eq!(m.unit_dmg(UnitKind::Ram), 55.0 * 1.25);
        assert_eq!(m.unit_armor(UnitKind::Ram), 0.0);
        assert_eq!(train_speed(1), 1.0);
        assert!((train_speed(3) - 1.4).abs() < 1e-6);
        assert_eq!(train_speed(20), 2.0);
        assert_eq!(sell_price(0.0), 50.0);
        assert_eq!(buy_price(100.0), 320.0);
        assert_eq!(AiLevel::Hard.def().wave_need_cap(), 24);
        assert_eq!(AiLevel::Normal.def().wave_need_cap(), 14);
        assert_eq!(AiLevel::Easy.def().wave_need_cap(), 7);
        assert_eq!(dir5(10.0, 0.0), Dir5::Right);
        assert_eq!(dir5(0.0, -10.0), Dir5::Up);
        assert_eq!(dir5(-1.0, 10.0), Dir5::Down);
        assert!((BuildingKind::Tower.def().muzzle() - 143.84).abs() < 1e-3);
        assert_eq!(KingdomColor::Red.kingdom_name(), "Regno Rosso");
    }
}
