use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use asset::data::GameData;
use asset::ui::{Layout, StatFilter};
use protocol::You;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, MouseEvent, WheelEvent};

use crate::dom::{Listener, append, clear, element, listen, on, rich_markup, set_style, show};
use crate::frame::{Frame, Instance, Template, View};
use crate::ui::{Events, UiEvent, owned_tooltip};

const SHOP: &str = "ItemShop/ItemShop";
const ALL_ITEMS: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems";
const ALL_ITEMS_PANE: &str =
    "ItemShop/ItemShop/ItemShop_TabView_AllItems/ItemShop_AllItems_ViewPane";
const DETAILS: &str = "ItemShop/ItemShop/ItemShop_ItemDetails";
const TEMPLATE: &str = "ItemShop/ItemShop/ItemShop_ItemPageTemplate";
const TAB: &str = "ItemShop/ItemShop/ItemShop_TabButton";
const ALL_ITEMS_TAB: &str = "ItemShop/ItemShop/ItemShop_AllItemsTab";
const BLOCK: &str = "ItemShop/ItemShop/Backdrop/ItemShop_BackdropFrame_InputBlockRegion";
const CLOSE: &str = "ItemShop/ItemShop/ItemShop_CloseButton";
const PURCHASE: &str = "ItemShop/ItemShop/ItemShop_PurchaseButton";
const SELL: &str = "ItemShop/ItemShop/ItemShop_SellButton";
const UNDO: &str = "ItemShop/ItemShop/ItemShop_UndoButton";
const GOLD_TEXT: &str = "ItemShop/ItemShop/Footer/ItemShop_Footer_GoldText";
const GOLD_ICON: &str = "ItemShop/ItemShop/Footer/ItemShop_Footer_GoldIcon";
const SEARCH_PLACEHOLDER: &str =
    "ItemShop/ItemShop/ItemShop_SearchBar/ItemShop_SearchBar_Default_Text";
const FILTERS: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/filter";
const ALL_FILTER: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/filter/ItemShop_AllButton";
const POPULAR_FILTER: &str =
    "ItemShop/ItemShop/ItemShop_TabView_AllItems/filter/ItemShop_PopularButton";
const INVERT: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/filter/ItemShop_InvertButton";
const CLEAR_STATS: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/statfilters/DisableBtn";
const CLASS_FILTERS: [(&str, u8); 6] = [
    ("ItemShop_AttackButton", FIGHTER),
    ("ItemShop_MarksmanButton", MARKSMAN),
    ("ItemShop_AssassinButton", ASSASSIN),
    ("ItemShop_MagicButton", MAGE),
    ("ItemShop_DefenseButton", TANK),
    ("ItemShop_UtilityButton", SUPPORT),
];
const FIGHTER: u8 = 1;
const MARKSMAN: u8 = 2;
const ASSASSIN: u8 = 4;
const TANK: u8 = 8;
const MAGE: u8 = 16;
const SUPPORT: u8 = 32;
const ROLES: [(&str, u8); 6] = [
    ("fighter", FIGHTER),
    ("marksman", MARKSMAN),
    ("assassin", ASSASSIN),
    ("tank", TANK),
    ("mage", MAGE),
    ("support", SUPPORT),
];
const SCISSOR: &str =
    "ItemShop/ItemShop/ItemShop_TabView_AllItems/scrolling/ItemShop_AllItems_ScissorRegion";
const GRID: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/scrolling/Grid";
const SLIDER: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/scrolling/ItemShop_AllItems_Slider/ItemShop_AllItems_Slider_Slider";
const SLIDER_BAR: &str = "ItemShop/ItemShop/ItemShop_TabView_AllItems/scrolling/ItemShop_AllItems_Slider/ItemShop_AllItems_Slider_BarHitRegion";
const HEADER_REGION: &str = "ItemShop_AllItems_HeaderRegion";
const HEADER_TEXT: &str = "ItemShop_AllItems_HeaderText";
const ITEM_REGION: &str = "ItemShop_AllItems_ItemRegion";
const COLUMNS: usize = 10;
const TIERS: [(u8, &str); 4] = [
    (1, "shop_group_starter"),
    (2, "shop_group_basic"),
    (4, "shop_group_epic"),
    (5, "shop_group_legendary"),
];
const TREE_REGION: &str = "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_BuildTreeDisplayRegion";
const TREE: &str = "ItemShop/ItemShop/ItemShop_ItemDetails/Tree";
const TREE_PITCH: f32 = 110.0;
const BRACKET: f32 = 16.0;
const LINE: f32 = 2.0;
const BUILDS_INTO: &str = "ItemShop/ItemShop/BuildsInto";
const BUILDS_INTO_SLOTS: usize = 7;
const DROPLIST: &str = "ItemShop/ItemShop/BuildsInto/ItemShop_BuildsInto_Droplist";
const OVERFLOW: &str = "ItemShop/ItemShop/BuildsInto/ItemShop_BuildsInto_OverflowButton";
const DESCRIPTION_ICON: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_ItemDescription_ItemIcon";
const DESCRIPTION_TITLE: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_ItemDescription_TitleText";
const DESCRIPTION_GOLD: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_ItemDescription_GoldIcon";
const DESCRIPTION_COST: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_ItemDescription_CostText";
const DESCRIPTION_BODY: &str = "ItemShop/ItemShop/ItemShop_ItemDetails/ItemShop_ItemDetails_ViewPane/ItemShop_ItemDescription_Body";
const DESCRIPTION_SCISSOR: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/scrolling/ItemShop_ItemDetails_ScissorRegion";
const DESCRIPTION_SCROLL: &str = "ItemShop/ItemShop/ItemShop_ItemDetails/scrolling/Description";
const DESCRIPTION_SLIDER: &str =
    "ItemShop/ItemShop/ItemShop_ItemDetails/scrolling/ItemDetails_Slider/ItemDetails_Slider_Slider";
const DESCRIPTION_BAR: &str = "ItemShop/ItemShop/ItemShop_ItemDetails/scrolling/ItemDetails_Slider/ItemDetails_Slider_BarHitRegion";
const INVENTORY_PANEL: &str = "ItemShop/ItemShop/ItemShop_Inventory_Panel";
const INVENTORY_SLOTS: usize = 7;
const PANELS: [PanelKind; 3] = [
    PanelKind {
        scene: "ItemShop/ItemShop/ItemShop_ConsumablesPanel",
        prefix: "ItemShop_ConsumablesPanel",
        pin: "ConsumablesPanel_PinBtn",
        slots: 12,
    },
    PanelKind {
        scene: "ItemShop/ItemShop/ItemShop_BootsPanel",
        prefix: "ItemShop_BootsPanel",
        pin: "ItemShop_BootsPanel_PinButton",
        slots: 9,
    },
    PanelKind {
        scene: INVENTORY_PANEL,
        prefix: "ItemShop_Inventory_Panel",
        pin: "Inventory_Panel_PinButton",
        slots: INVENTORY_SLOTS,
    },
];
const CONSUMABLES: usize = 0;
const BOOTS: usize = 1;
const INVENTORY: usize = 2;

const ICON: &str = "ItemShop_ItemPageTemplate_Item_Icon";
const FRAME: &str = "ItemShop_ItemPageTemplate_Item_Frame";
const FRAME_HOVER: &str = "ItemShop_ItemPageTemplate_Item_FrameHover";
const HOVER: &str = "ItemShop_ItemPageTemplate_Item_HoverMajor";
const SELECTED_MAJOR: &str = "ItemShop_ItemPageTemplate_Item_SelectedMajor";
const SELECTED_MINOR: &str = "ItemShop_ItemPageTemplate_Item_SelectedMinor";
const UNPURCHASEABLE: &str = "ItemShop_ItemPageTemplate_Item_Unpurchaseable";
const PURCHASED: &str = "ItemShop_ItemPageTemplate_Item_Purchased";
const LOCKED: &str = "ItemShop_ItemPageTemplate_Item_Locked";
const COST: &str = "ItemShop_ItemPageTemplate_Item_CostText";
const COST_GREY: &str = "ItemShop_ItemPageTemplate_Item_CostTextUnpurchaseable";
const COST_SELECTED: &str = "ItemShop_ItemPageTemplate_Item_CostTextSelected";
const CLICK: &str = "ItemShop_ItemPageTemplate_Item_ClickRegion";
const INVENTORY_ICON: &str = "ItemShop_ItemPageTemplate_Item_IconInventory";
const INVENTORY_FRAME: &str = "ItemShop_ItemPageTemplate_Item_FrameInventory";
const INVENTORY_HOVER: &str = "ItemShop_ItemPageTemplate_Item_HoverInventory";
const INVENTORY_SELECTED: &str = "ItemShop_ItemPageTemplate_Item_SelectedInventory";
const INVENTORY_CLICK: &str = "ItemShop_ItemPageTemplate_Item_ClickRegionInventory";
const COSTS: [&str; 3] = [COST, COST_GREY, COST_SELECTED];

const MAJOR: Look = Look {
    parts: &[
        ICON,
        FRAME,
        FRAME_HOVER,
        HOVER,
        SELECTED_MAJOR,
        UNPURCHASEABLE,
        PURCHASED,
        LOCKED,
        COST,
        COST_GREY,
        COST_SELECTED,
        CLICK,
    ],
    hover: &[FRAME_HOVER, HOVER],
    selected: SELECTED_MAJOR,
    cost: true,
};
const MINOR: Look = Look {
    parts: &[
        ICON,
        FRAME,
        FRAME_HOVER,
        SELECTED_MINOR,
        UNPURCHASEABLE,
        PURCHASED,
        LOCKED,
        COST,
        COST_GREY,
        COST_SELECTED,
        CLICK,
    ],
    hover: &[FRAME_HOVER],
    selected: SELECTED_MINOR,
    cost: true,
};
const BARE: Look = Look {
    parts: &[
        ICON,
        FRAME,
        FRAME_HOVER,
        SELECTED_MINOR,
        UNPURCHASEABLE,
        PURCHASED,
        CLICK,
    ],
    hover: &[FRAME_HOVER],
    selected: SELECTED_MINOR,
    cost: false,
};

struct Look {
    parts: &'static [&'static str],
    hover: &'static [&'static str],
    selected: &'static str,
    cost: bool,
}

struct PanelKind {
    scene: &'static str,
    prefix: &'static str,
    pin: &'static str,
    slots: usize,
}

impl PanelKind {
    fn part(&self, part: &str) -> String {
        format!("{}/{}_{part}", self.scene, self.prefix)
    }

    fn slot(&self, slot: usize) -> String {
        format!("{}/Slots/{}_Slot{slot}", self.scene, self.prefix)
    }

    fn slot_count(&self, slot: usize) -> String {
        format!("{}_CountText", self.slot(slot))
    }

    fn pin(&self) -> String {
        format!("{}/{}", self.scene, self.pin)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Action {
    Class(Option<u8>),
    Stat(usize),
    ClearStats,
    Invert,
    Select(u32),
    Describe(u32),
    Slot(usize),
    Buy(u32),
    BuySelected,
    Sell(usize),
    SellSelected,
    Undo,
    Scroll(Scroller, f32),
    Press(String),
    Expand(usize, bool),
    Pin(usize),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scroller {
    Grid,
    Description,
}

type Actions = Rc<RefCell<Vec<Action>>>;

#[derive(Debug, Clone, Default, PartialEq)]
struct State {
    class: Option<u8>,
    stats: Vec<bool>,
    inverted: bool,
    root: Option<u32>,
    described: Option<u32>,
    slot: Option<usize>,
    expanded: [bool; 3],
    pinned: [bool; 3],
}

#[derive(Debug, Clone, PartialEq)]
struct Painted {
    state: State,
    gold: u32,
    items: [Option<u32>; 6],
    stacks: [u8; 6],
    can_shop: bool,
    can_undo: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Status {
    cost: u32,
    affordable: bool,
    locked: bool,
    owned: bool,
    selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Branch {
    item: u32,
    depth: usize,
    parent: Option<usize>,
    span: [f32; 2],
    owned: bool,
}

struct Scroll {
    content: HtmlElement,
    offset: f32,
    height: f32,
    visible: f32,
    slider: &'static str,
    bar: &'static str,
}

impl Scroll {
    fn new(
        frame: &mut Frame,
        name: &str,
        scissor: &str,
        actions: &Actions,
        scroller: Scroller,
    ) -> Option<Self> {
        frame.add(name, scissor, "frame-scroll");
        let view = frame.get(name)?.clone();
        let content = element("div", "frame-scroll-content");
        append(&view, &content);
        let actions = actions.clone();
        on(&view, "wheel", move |event| {
            if let Some(wheel) = event.dyn_ref::<WheelEvent>() {
                event.prevent_default();
                actions
                    .borrow_mut()
                    .push(Action::Scroll(scroller, wheel.delta_y() as f32));
            }
        });
        let visible = frame.rect(scissor)?.size[1];
        let (slider, bar) = match scroller {
            Scroller::Grid => (SLIDER, SLIDER_BAR),
            Scroller::Description => (DESCRIPTION_SLIDER, DESCRIPTION_BAR),
        };
        Some(Self {
            content,
            offset: 0.0,
            height: 0.0,
            visible,
            slider,
            bar,
        })
    }

    fn by(&mut self, delta: f32) {
        self.offset = (self.offset + delta).clamp(0.0, (self.height - self.visible).max(0.0));
    }

    fn resize(&mut self, height: f32) {
        self.height = height;
        self.by(0.0);
    }

    fn place(&self, frame: &Frame) {
        set_style(
            &self.content,
            "transform",
            &format!("translateY({:.1}px)", -self.offset),
        );
        let overflow = self.height > self.visible;
        frame.show(self.slider, overflow);
        let (Some(thumb), Some(bar)) = (frame.rect(self.slider), frame.rect(self.bar)) else {
            return;
        };
        let travel = (bar.size[1] - thumb.size[1]).max(0.0);
        let fraction = self.offset / (self.height - self.visible).max(1.0);
        frame.move_to(
            self.slider,
            [thumb.position[0], bar.position[1] + travel * fraction],
        );
    }
}

struct Cell {
    item: Option<u32>,
    view: Instance,
    _listeners: Vec<Listener>,
}

struct Context {
    data: Rc<GameData>,
    actions: Actions,
    tooltip: HtmlElement,
}

impl Context {
    fn cell(
        &self,
        parent: &HtmlElement,
        template: &Template,
        look: &Look,
        item: Option<u32>,
        click: Action,
    ) -> Cell {
        let view = template.instance(parent);
        let item = item.and_then(|id| self.data.item(id));
        let mut listeners = Vec::new();
        if let Some(region) = view.interactive(CLICK) {
            listeners.extend(hover(&region, &view, look.hover));
            if let Some(item) = item {
                listeners.extend(owned_tooltip(
                    &region,
                    &self.tooltip,
                    &item.name,
                    &item.description,
                ));
                listeners.extend([
                    bind(&region, "click", &self.actions, click),
                    bind(&region, "contextmenu", &self.actions, Action::Buy(item.id)),
                    bind(&region, "dblclick", &self.actions, Action::Buy(item.id)),
                ]);
            }
        }
        if let Some(item) = item {
            view.image(ICON, &format!("assets/icons/{}", item.icon));
        }
        Cell {
            item: item.map(|item| item.id),
            view,
            _listeners: listeners,
        }
    }
}

struct Panel {
    cells: Vec<(usize, Cell)>,
    closed_x: f32,
}

pub struct Shop {
    frame: Frame,
    data: Rc<GameData>,
    context: Context,
    events: Events,
    actions: Actions,
    strings: BTreeMap<String, String>,
    filters: Vec<StatFilter>,
    major: Template,
    minor: Template,
    bare: Template,
    inventory_template: Template,
    header: Template,
    open: bool,
    scale: f32,
    state: State,
    grid: Option<Scroll>,
    description: Option<Scroll>,
    headers: Vec<Instance>,
    cells: Vec<Cell>,
    builds: Vec<Cell>,
    overflow: usize,
    tree: Vec<(Branch, Cell)>,
    panels: Vec<Panel>,
    origins: Origins,
    painted: Option<Painted>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Origins {
    scissor: [f32; 2],
    header: [f32; 2],
    item: [f32; 2],
    icon: [f32; 2],
    inventory_icon: [f32; 2],
}

impl Shop {
    pub fn new(
        parent: &Element,
        layout: &Layout,
        data: Rc<GameData>,
        events: &Events,
        tooltip: &HtmlElement,
        role: &str,
    ) -> Self {
        let mut frame = Frame::new(parent, layout, &[SHOP]);
        frame.show_scene(SHOP, false);
        frame.show_scene(ALL_ITEMS, true);
        frame.show_scene(DETAILS, true);
        frame.show_scene(TEMPLATE, false);
        frame.show_scene(DROPLIST, false);
        let actions: Actions = Rc::default();
        let origin = |name: &str| {
            layout
                .element(name)
                .map_or([0.0; 2], |element| element.rect.position)
        };
        let cell = origin(&format!("{TEMPLATE}/{CLICK}"));
        let icon = origin(&format!("{TEMPLATE}/{ICON}"));
        let inventory_cell = origin(&format!("{TEMPLATE}/{INVENTORY_CLICK}"));
        let inventory_icon = origin(&format!("{TEMPLATE}/{INVENTORY_ICON}"));
        let origins = Origins {
            scissor: origin(SCISSOR),
            header: origin(&format!("{ALL_ITEMS_PANE}/{HEADER_REGION}")),
            item: origin(&format!("{ALL_ITEMS_PANE}/{ITEM_REGION}")),
            icon: [icon[0] - cell[0], icon[1] - cell[1]],
            inventory_icon: [
                inventory_icon[0] - inventory_cell[0],
                inventory_icon[1] - inventory_cell[1],
            ],
        };
        let template = |look: &Look| Template::new(layout, TEMPLATE, CLICK, look.parts);
        let major = template(&MAJOR);
        let minor = template(&MINOR);
        let bare = template(&BARE);
        let inventory_template = Template::new(
            layout,
            TEMPLATE,
            INVENTORY_CLICK,
            &[
                INVENTORY_ICON,
                INVENTORY_FRAME,
                INVENTORY_HOVER,
                INVENTORY_SELECTED,
                INVENTORY_CLICK,
            ],
        );
        let header = Template::new(layout, ALL_ITEMS_PANE, HEADER_REGION, &[HEADER_TEXT]);
        let grid = Scroll::new(&mut frame, GRID, SCISSOR, &actions, Scroller::Grid);
        let description = Scroll::new(
            &mut frame,
            DESCRIPTION_SCROLL,
            DESCRIPTION_SCISSOR,
            &actions,
            Scroller::Description,
        );
        frame.add(TREE, TREE_REGION, "frame-group");
        let strings = layout.strings.clone();
        let mut shop = Self {
            frame,
            context: Context {
                data: data.clone(),
                actions: actions.clone(),
                tooltip: tooltip.clone(),
            },
            data,
            events: events.clone(),
            actions,
            strings,
            filters: layout.filters.clone(),
            major,
            minor,
            bare,
            inventory_template,
            header,
            open: false,
            scale: 1.0,
            state: State {
                class: ROLES
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(role))
                    .map(|(_, class)| *class),
                stats: vec![false; layout.filters.len()],
                ..State::default()
            },
            grid,
            description,
            headers: Vec::new(),
            cells: Vec::new(),
            builds: Vec::new(),
            overflow: 0,
            tree: Vec::new(),
            panels: Vec::new(),
            origins,
            painted: None,
        };
        shop.build_chrome();
        shop.build_grid();
        shop.build_panels();
        shop.arrange();
        if let Some(scroll) = &shop.description {
            scroll.place(&shop.frame);
        }
        shop
    }

    fn label(&self, key: &str) -> &str {
        self.strings.get(key).map_or("", |value| value.trim())
    }

    fn button(&self, button: &str, action: Action) {
        self.frame.show_button(button, true);
        let actions = self.actions.clone();
        let name = button.to_owned();
        self.frame.on_click(button, move |_| {
            let mut actions = actions.borrow_mut();
            actions.push(Action::Press(name.clone()));
            actions.push(action.clone());
        });
    }

    fn build_chrome(&mut self) {
        let frame = &mut self.frame;
        frame.interactive(BLOCK);
        frame.instance(TAB, ALL_ITEMS_TAB, [tab_offset(frame, 1), 0.0]);
        frame.show_button(ALL_ITEMS_TAB, true);
        frame.select(ALL_ITEMS_TAB, true);
        let label = self
            .strings
            .get("shop_tab_allitems")
            .map_or("", String::as_str);
        for (part, selected) in [
            ("ItemShop_TabButton_Text", false),
            ("ItemShop_TabButton_HoverText", true),
        ] {
            let name = format!("{ALL_ITEMS_TAB}/{part}");
            frame.text(&name, label);
            frame.show(&name, selected);
        }
        for name in [GOLD_TEXT, GOLD_ICON, SEARCH_PLACEHOLDER] {
            frame.show(name, true);
        }
        frame.text(
            SEARCH_PLACEHOLDER,
            self.strings
                .get("itemshop_search_placeholder")
                .map_or("", String::as_str),
        );
        self.button(CLOSE, Action::Close);
        self.button(PURCHASE, Action::BuySelected);
        self.button(SELL, Action::SellSelected);
        self.button(UNDO, Action::Undo);
        self.button(ALL_FILTER, Action::Class(None));
        self.frame.show_button(POPULAR_FILTER, true);
        self.frame.enable(POPULAR_FILTER, false);
        self.button(INVERT, Action::Invert);
        self.button(CLEAR_STATS, Action::ClearStats);
        for (name, class) in CLASS_FILTERS {
            self.button(&format!("{FILTERS}/{name}"), Action::Class(Some(class)));
        }
        let buttons: Vec<String> = self
            .filters
            .iter()
            .map(|filter| filter.button.clone())
            .collect();
        for (index, button) in buttons.iter().enumerate() {
            self.button(button, Action::Stat(index));
        }
    }

    fn build_grid(&mut self) {
        let Some(grid) = &self.grid else {
            return;
        };
        let content = grid.content.clone();
        self.headers = TIERS
            .iter()
            .map(|(_, tier)| {
                let view = self.header.instance(&content);
                view.text(HEADER_TEXT, self.label(tier));
                view
            })
            .collect();
        let mut items: Vec<u32> = self.data.items.iter().map(|item| item.id).collect();
        items.sort_by_key(|id| self.data.item(*id).map(|item| (item.total_price, item.id)));
        self.cells = items
            .into_iter()
            .map(|id| {
                self.context
                    .cell(&content, &self.major, &MAJOR, Some(id), Action::Select(id))
            })
            .collect();
    }

    fn build_panels(&mut self) {
        let mut panels = Vec::new();
        for (index, kind) in PANELS.iter().enumerate() {
            let pin = kind.pin();
            self.button(&pin, Action::Pin(index));
            if let Some(region) = self.frame.interactive(&kind.part("ExpandedRegion")) {
                hover_panel(&region, &self.actions, index);
            }
            let Some(parent) = self
                .frame
                .get(&kind.slot(0))
                .and_then(|slot| slot.parent_element())
                .and_then(|parent| parent.dyn_into::<HtmlElement>().ok())
            else {
                continue;
            };
            let items: Vec<Option<u32>> = match index {
                CONSUMABLES => self
                    .data
                    .quick_buy
                    .consumables
                    .iter()
                    .copied()
                    .map(Some)
                    .collect(),
                BOOTS => self
                    .data
                    .quick_buy
                    .boots
                    .iter()
                    .copied()
                    .map(Some)
                    .collect(),
                _ => vec![None; kind.slots],
            };
            let cells = (0..kind.slots)
                .filter_map(|slot| {
                    let rect = self.frame.rect(&kind.slot(slot))?;
                    let item = if index == INVENTORY {
                        None
                    } else {
                        Some(items.get(slot).copied().flatten()?)
                    };
                    let cell = if index == INVENTORY {
                        self.inventory_cell(&parent, slot)
                    } else {
                        self.context
                            .cell(&parent, &self.major, &MAJOR, item, Action::Select(item?))
                    };
                    let inset = if index == INVENTORY {
                        self.origins.inventory_icon
                    } else {
                        self.origins.icon
                    };
                    place(
                        &cell.view.root,
                        [rect.position[0] - inset[0], rect.position[1] - inset[1]],
                    );
                    if let Some(region) = cell.view.get(if index == INVENTORY {
                        INVENTORY_CLICK
                    } else {
                        CLICK
                    }) {
                        hover_panel(region, &self.actions, index);
                    }
                    Some((slot, cell))
                })
                .collect();
            panels.push(Panel {
                cells,
                closed_x: self
                    .frame
                    .rect(&kind.part("Closed"))
                    .map_or(0.0, |rect| rect.position[0]),
            });
        }
        self.panels = panels;
    }

    fn inventory_cell(&self, parent: &HtmlElement, slot: usize) -> Cell {
        let view = self.inventory_template.instance(parent);
        let mut listeners = Vec::new();
        if let Some(region) = view.interactive(INVENTORY_CLICK) {
            listeners.extend(hover(&region, &view, &[INVENTORY_HOVER]));
            listeners.extend([
                bind(&region, "click", &self.actions, Action::Slot(slot)),
                bind(&region, "contextmenu", &self.actions, Action::Sell(slot)),
            ]);
        }
        Cell {
            item: None,
            view,
            _listeners: listeners,
        }
    }

    pub fn resize(&mut self, height: f32) {
        self.scale = height / 1200.0;
        self.frame.resize(height);
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.frame.show_scene(SHOP, self.open);
    }

    pub fn open(&self) -> bool {
        self.open
    }

    pub fn update(&mut self, you: &You) {
        let actions: Vec<Action> = self.actions.borrow_mut().drain(..).collect();
        for action in actions {
            self.apply(action, you);
        }
        if !self.open {
            return;
        }
        if self
            .painted
            .as_ref()
            .is_some_and(|painted| painted.items != you.items)
        {
            self.build_tree(you);
        }
        let painted = Painted {
            state: self.state.clone(),
            gold: you.gold.max(0.0) as u32,
            items: you.items,
            stacks: you.stacks,
            can_shop: you.can_shop,
            can_undo: you.can_undo,
        };
        if self.painted.as_ref() != Some(&painted) {
            self.paint(&painted);
            self.painted = Some(painted);
        }
    }

    fn apply(&mut self, action: Action, you: &You) {
        let state = &mut self.state;
        match action {
            Action::Class(class) => {
                state.class = class;
                self.arrange();
            }
            Action::Stat(index) => {
                if let Some(stat) = state.stats.get_mut(index) {
                    *stat = !*stat;
                }
                self.arrange();
            }
            Action::ClearStats => {
                state.stats.iter_mut().for_each(|stat| *stat = false);
                self.arrange();
            }
            Action::Invert => {
                state.inverted = !state.inverted;
                self.arrange();
            }
            Action::Select(item) => {
                state.root = Some(item);
                state.described = Some(item);
                state.slot = None;
                self.build_tree(you);
            }
            Action::Describe(item) => state.described = Some(item),
            Action::Slot(slot) => {
                state.slot = Some(slot);
                if let Some(item) = you.items.get(slot).copied().flatten() {
                    state.root = Some(item);
                    state.described = Some(item);
                    self.build_tree(you);
                }
            }
            Action::Buy(item) => self.events.borrow_mut().push(UiEvent::Buy(item)),
            Action::BuySelected => {
                if let Some(root) = state.root {
                    let orders = orders(&self.data, root, you);
                    self.events
                        .borrow_mut()
                        .extend(orders.into_iter().map(UiEvent::Buy));
                }
            }
            Action::Sell(slot) => {
                if you.items.get(slot).copied().flatten().is_some() {
                    self.events.borrow_mut().push(UiEvent::Sell(slot as u8));
                }
            }
            Action::Undo => {
                if you.can_undo {
                    self.events.borrow_mut().push(UiEvent::Undo);
                }
            }
            Action::SellSelected => {
                if let Some(slot) = state.slot.take() {
                    self.events.borrow_mut().push(UiEvent::Sell(slot as u8));
                }
            }
            Action::Scroll(scroller, delta) => {
                let scale = self.scale.max(0.1);
                let scroll = match scroller {
                    Scroller::Grid => self.grid.as_mut(),
                    Scroller::Description => self.description.as_mut(),
                };
                if let Some(scroll) = scroll {
                    scroll.by(delta / scale);
                    scroll.place(&self.frame);
                }
            }
            Action::Expand(panel, expanded) => {
                if let Some(slot) = state.expanded.get_mut(panel) {
                    *slot = expanded;
                }
            }
            Action::Pin(panel) => {
                if let Some(pinned) = state.pinned.get_mut(panel) {
                    *pinned = !*pinned;
                    self.frame.select(&PANELS[panel].pin(), *pinned);
                }
            }
            Action::Press(button) => {
                if let Some(sound) = self.frame.sound(&button) {
                    self.events.borrow_mut().push(UiEvent::Sound(sound));
                }
            }
            Action::Close => {
                if self.open {
                    self.events.borrow_mut().push(UiEvent::ToggleShop);
                }
            }
        }
    }

    fn arrange(&mut self) {
        let frame = &self.frame;
        frame.select(ALL_FILTER, self.state.class.is_none());
        for (name, class) in CLASS_FILTERS {
            frame.select(
                &format!("{FILTERS}/{name}"),
                self.state.class == Some(class),
            );
        }
        for (filter, selected) in self.filters.iter().zip(&self.state.stats) {
            frame.select(&filter.button, *selected);
        }
        frame.select(CLEAR_STATS, !self.state.stats.contains(&true));
        frame.select(INVERT, self.state.inverted);
        let Some(grid) = &mut self.grid else {
            return;
        };
        let origins = self.origins;
        let header_x = origins.header[0] - origins.scissor[0];
        let item_x = origins.item[0] - origins.scissor[0];
        let header_height = origins.item[1] - origins.header[1];
        let pitch = [57.0, 76.0];
        let mut tiers: Vec<usize> = (0..TIERS.len()).collect();
        if self.state.inverted {
            tiers.reverse();
        }
        let mut y = 0.0;
        let mut shown = vec![false; self.cells.len()];
        for tier in tiers {
            let mut members: Vec<usize> = self
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| {
                    cell.item
                        .and_then(|id| self.data.item(id))
                        .is_some_and(|item| {
                            TIERS[tier].0 == item.tier && matches(item, &self.state, &self.filters)
                        })
                })
                .map(|(index, _)| index)
                .collect();
            if self.state.inverted {
                members.reverse();
            }
            let header = &self.headers[tier];
            show(&header.root, !members.is_empty());
            if members.is_empty() {
                continue;
            }
            place(&header.root, [header_x, y]);
            y += header_height;
            for (index, member) in members.iter().enumerate() {
                shown[*member] = true;
                place(
                    &self.cells[*member].view.root,
                    [
                        item_x + (index % COLUMNS) as f32 * pitch[0],
                        y + (index / COLUMNS) as f32 * pitch[1],
                    ],
                );
            }
            y += members.len().div_ceil(COLUMNS) as f32 * pitch[1] + (42.0 - header_height);
        }
        for (cell, shown) in self.cells.iter().zip(shown) {
            show(&cell.view.root, shown);
        }
        grid.offset = 0.0;
        grid.resize(y);
        grid.place(&self.frame);
    }

    fn build_tree(&mut self, you: &You) {
        let Some(container) = self.frame.get(TREE).cloned() else {
            return;
        };
        clear(&container);
        self.tree.clear();
        self.builds.clear();
        self.overflow = 0;
        let (Some(root), Some(region)) = (self.state.root, self.frame.rect(TREE_REGION)) else {
            return;
        };
        let branches = recipe_tree(&self.data, root, &you.items, region.size[0]);
        let depth = branches
            .iter()
            .map(|branch| branch.depth)
            .max()
            .unwrap_or(0);
        let cell_size = [57.0, 76.0];
        let top = ((region.size[1] - depth as f32 * TREE_PITCH - cell_size[1]) / 2.0).max(0.0);
        let position = |branch: &Branch| {
            [
                (branch.span[0] + branch.span[1]) / 2.0 - cell_size[0] / 2.0,
                top + branch.depth as f32 * TREE_PITCH,
            ]
        };
        for (index, branch) in branches.iter().enumerate() {
            let children: Vec<&Branch> = branches
                .iter()
                .filter(|child| child.parent == Some(index))
                .collect();
            if let (Some(first), Some(last)) = (children.first(), children.last()) {
                let [x, y] = position(branch);
                let center = x + cell_size[0] / 2.0;
                let bracket = y + TREE_PITCH - BRACKET + self.origins.icon[1] - LINE;
                let stem = y + cell_size[1] - 4.0;
                line(
                    &container,
                    [center - LINE / 2.0, stem],
                    [LINE, bracket - stem],
                );
                let left = position(first)[0] + cell_size[0] / 2.0;
                let right = position(last)[0] + cell_size[0] / 2.0;
                line(
                    &container,
                    [left - LINE / 2.0, bracket],
                    [right - left + LINE, LINE],
                );
                for child in &children {
                    let center = position(child)[0] + cell_size[0] / 2.0;
                    line(&container, [center - LINE / 2.0, bracket], [LINE, BRACKET]);
                }
            }
        }
        for branch in branches {
            let cell = self.context.cell(
                &container,
                &self.minor,
                &MINOR,
                Some(branch.item),
                Action::Describe(branch.item),
            );
            place(&cell.view.root, position(&branch));
            self.tree.push((branch, cell));
        }
        self.build_builds_into(&container, root, region.position);
    }

    fn build_builds_into(&mut self, container: &HtmlElement, root: u32, origin: [f32; 2]) {
        let parents: Vec<u32> = self
            .data
            .items
            .iter()
            .filter(|item| item.from.contains(&root))
            .map(|item| item.id)
            .collect();
        let listed = if parents.len() > BUILDS_INTO_SLOTS {
            BUILDS_INTO_SLOTS - 1
        } else {
            BUILDS_INTO_SLOTS
        };
        self.builds = parents
            .iter()
            .take(listed)
            .enumerate()
            .filter_map(|(index, item)| {
                let slot = self.frame.rect(&format!(
                    "{BUILDS_INTO}/ItemShop_BuildsInto_Slot{}",
                    index + 1
                ))?;
                let cell = self.context.cell(
                    container,
                    &self.bare,
                    &BARE,
                    Some(*item),
                    Action::Select(*item),
                );
                place(
                    &cell.view.root,
                    [
                        slot.position[0] - self.origins.icon[0] - origin[0],
                        slot.position[1] - self.origins.icon[1] - origin[1],
                    ],
                );
                Some(cell)
            })
            .collect();
        self.overflow = parents.len().saturating_sub(listed);
    }

    fn paint(&mut self, painted: &Painted) {
        let data = &self.data;
        let state = &painted.state;
        let status = |item: u32, owned: bool, selected: bool| {
            let cost = data
                .purchase(item, &painted.items)
                .map_or(0, |purchase| purchase.cost);
            Status {
                cost,
                affordable: painted.gold >= cost,
                locked: !owned && locked(data, item, painted),
                owned,
                selected,
            }
        };
        let free = self.label("shop_zerocost_label").to_owned();
        for cell in &self.cells {
            if let Some(item) = cell.item {
                paint(
                    &cell.view,
                    status(item, false, state.root == Some(item)),
                    &MAJOR,
                    &free,
                );
            }
        }
        for (branch, cell) in &self.tree {
            paint(
                &cell.view,
                status(
                    branch.item,
                    branch.owned,
                    state.described == Some(branch.item),
                ),
                &MINOR,
                &free,
            );
        }
        for (cell, item) in self
            .builds
            .iter()
            .filter_map(|cell| Some((cell, cell.item?)))
        {
            paint(&cell.view, status(item, false, false), &BARE, &free);
        }
        for index in 0..BUILDS_INTO_SLOTS {
            self.frame.show(
                &format!(
                    "{BUILDS_INTO}/ItemShop_BuildsInto_EmptyStates/ItemShop_BuildsInto_EmptyState{}",
                    index + 1
                ),
                index >= self.builds.len()
                    && !(self.overflow > 0 && index == BUILDS_INTO_SLOTS - 1),
            );
        }
        self.frame.show_button(OVERFLOW, self.overflow > 0);
        if self.overflow > 0 {
            let more = format!("+{}", self.overflow);
            for part in [
                "ItemShop_BuildsInto_Overflow_Text",
                "ItemShop_BuildsInto_Overflow_TextHover",
            ] {
                self.frame.text(&format!("{OVERFLOW}/{part}"), &more);
            }
        }
        self.paint_purchase(painted);
        self.paint_description(painted);
        self.paint_panels(painted, &free);
        let frame = &self.frame;
        frame.text(GOLD_TEXT, &painted.gold.to_string());
        let sellable = painted.can_shop
            && state
                .slot
                .and_then(|slot| painted.items.get(slot).copied().flatten())
                .is_some();
        frame.enable(SELL, sellable);
        label_states(frame, SELL, "ItemShop_SellButton", sellable);
        frame.enable(UNDO, painted.can_undo);
        label_states(frame, UNDO, "ItemShop_UndoButton", painted.can_undo);
    }

    fn paint_purchase(&self, painted: &Painted) {
        let frame = &self.frame;
        let Some(root) = painted.state.root else {
            frame.show_button(PURCHASE, false);
            for part in PURCHASE_TEXTS {
                frame.show(&format!("{PURCHASE}/{part}"), false);
            }
            return;
        };
        let key = purchase_label(&self.data, root, painted);
        let enabled = matches!(
            key,
            "shop_autobuild_can_purchase" | "shop_autobuild_can_purchase_components"
        );
        frame.show_button(PURCHASE, true);
        frame.enable(PURCHASE, enabled);
        let label = self.label(key);
        for part in PURCHASE_TEXTS {
            let name = format!("{PURCHASE}/{part}");
            frame.text(&name, label);
            frame.show(
                &name,
                if enabled {
                    part == PURCHASE_TEXTS[0]
                } else {
                    part == PURCHASE_TEXTS[1]
                },
            );
        }
    }

    fn paint_description(&mut self, painted: &Painted) {
        let item = painted.state.described.and_then(|id| self.data.item(id));
        let frame = &self.frame;
        for name in [
            DESCRIPTION_ICON,
            DESCRIPTION_TITLE,
            DESCRIPTION_GOLD,
            DESCRIPTION_COST,
        ] {
            frame.show(name, item.is_some());
        }
        let Some(item) = item else {
            if let Some(scroll) = &self.description {
                clear(&scroll.content);
            }
            return;
        };
        frame.image(DESCRIPTION_ICON, &format!("assets/icons/{}", item.icon));
        frame.text(DESCRIPTION_TITLE, &item.name);
        let cost = self
            .data
            .purchase(item.id, &painted.items)
            .map_or(item.total_price, |purchase| purchase.cost);
        frame.text(DESCRIPTION_COST, &cost.to_string());
        let (Some(body), Some(scroll)) = (
            frame.get(DESCRIPTION_BODY).cloned(),
            self.description.as_mut(),
        ) else {
            return;
        };
        if body.parent_element().as_ref() != Some(scroll.content.as_ref()) {
            append(&scroll.content, &body);
            set_style(&body, "left", "0px");
            set_style(&body, "top", "3px");
        }
        body.set_inner_html(&rich_markup(&item.description));
        show(&body, true);
        scroll.offset = 0.0;
        scroll.resize(body.offset_height() as f32 + 3.0);
        scroll.place(frame);
    }

    fn paint_panels(&self, painted: &Painted, free: &str) {
        for (index, (kind, panel)) in PANELS.iter().zip(&self.panels).enumerate() {
            let opened = painted.state.expanded[index] || painted.state.pinned[index];
            self.frame.show(&kind.part("Opened"), opened);
            self.frame.show(&kind.part("Closed"), !opened);
            for (slot, cell) in &panel.cells {
                let rect = self.frame.rect(&kind.slot(*slot));
                let visible = if index == INVENTORY {
                    opened
                } else {
                    opened || rect.is_some_and(|rect| rect.position[0] >= panel.closed_x)
                };
                show(&cell.view.root, visible);
                if index == INVENTORY {
                    let item = painted.items.get(*slot).copied().flatten();
                    let icon = item.and_then(|id| self.data.item(id));
                    cell.view.show(INVENTORY_ICON, icon.is_some());
                    if let Some(icon) = icon {
                        cell.view
                            .image(INVENTORY_ICON, &format!("assets/icons/{}", icon.icon));
                    }
                    cell.view.show(
                        INVENTORY_SELECTED,
                        item.is_some() && painted.state.slot == Some(*slot),
                    );
                    let count = painted.stacks.get(*slot).copied().unwrap_or(0);
                    let count_text = kind.slot_count(*slot);
                    self.frame.show(&count_text, count > 1);
                    self.frame.text(&count_text, &count.to_string());
                } else if let Some(item) = cell.item {
                    let cost = self
                        .data
                        .purchase(item, &painted.items)
                        .map_or(0, |purchase| purchase.cost);
                    paint(
                        &cell.view,
                        Status {
                            cost,
                            affordable: painted.gold >= cost,
                            locked: locked(&self.data, item, painted),
                            owned: false,
                            selected: painted.state.root == Some(item),
                        },
                        &MAJOR,
                        free,
                    );
                }
            }
        }
    }
}

const PURCHASE_TEXTS: [&str; 2] = [
    "ItemShop_PurchaseButton_Text",
    "ItemShop_PurchaseButton_DisabledText",
];

fn label_states(frame: &Frame, button: &str, prefix: &str, enabled: bool) {
    frame.show(&format!("{button}/{prefix}_Text"), enabled);
    frame.show(&format!("{button}/{prefix}_TextDisabled"), !enabled);
}

fn tab_offset(frame: &Frame, index: usize) -> f32 {
    frame
        .rect("ItemShop/ItemShop/Backdrop/ItemShop_KeyLine_Tabs")
        .map_or(0.0, |rect| rect.size[0] / 3.0 * index as f32)
}

fn place(view: &HtmlElement, [x, y]: [f32; 2]) {
    set_style(view, "left", &format!("{x}px"));
    set_style(view, "top", &format!("{y}px"));
}

fn line(parent: &HtmlElement, position: [f32; 2], size: [f32; 2]) {
    let view = element("div", "frame-line");
    place(&view, position);
    set_style(&view, "width", &format!("{}px", size[0].max(0.0)));
    set_style(&view, "height", &format!("{}px", size[1].max(0.0)));
    append(parent, &view);
}

fn bind(target: &HtmlElement, event: &str, actions: &Actions, action: Action) -> Listener {
    let actions = actions.clone();
    listen(target, event, move |event| {
        event.prevent_default();
        actions.borrow_mut().push(action.clone());
    })
}

fn hover(region: &HtmlElement, view: &Instance, parts: &[&str]) -> [Listener; 2] {
    let parts: Vec<HtmlElement> = parts
        .iter()
        .filter_map(|part| view.get(part).cloned())
        .collect();
    let entered = parts.clone();
    [
        listen(region, "mouseenter", move |_| {
            for part in &entered {
                show(part, true);
            }
        }),
        listen(region, "mouseleave", move |_| {
            for part in &parts {
                show(part, false);
            }
        }),
    ]
}

fn hover_panel(region: &HtmlElement, actions: &Actions, panel: usize) {
    let _ = region.set_attribute("data-panel", &panel.to_string());
    let entered = actions.clone();
    on(region, "mouseenter", move |_| {
        entered.borrow_mut().push(Action::Expand(panel, true));
    });
    let left = actions.clone();
    on(region, "mouseleave", move |event| {
        let staying = event
            .dyn_ref::<MouseEvent>()
            .and_then(MouseEvent::related_target)
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|target| target.get_attribute("data-panel"))
            == Some(panel.to_string());
        if !staying {
            left.borrow_mut().push(Action::Expand(panel, false));
        }
    });
}

fn paint(view: &Instance, status: Status, look: &Look, free: &str) {
    view.show(look.selected, status.selected);
    view.show(
        UNPURCHASEABLE,
        status.locked || status.owned || !status.affordable,
    );
    view.show(PURCHASED, status.owned);
    if !look.cost {
        return;
    }
    view.show(LOCKED, status.locked);
    let text = if status.cost == 0 {
        free.to_owned()
    } else {
        status.cost.to_string()
    };
    let variant = if status.selected {
        COST_SELECTED
    } else if status.affordable && !status.owned {
        COST
    } else {
        COST_GREY
    };
    for name in COSTS {
        view.show(name, !status.locked && name == variant);
        view.text(name, &text);
    }
}

fn matches(item: &asset::data::Item, state: &State, filters: &[StatFilter]) -> bool {
    state.class.is_none_or(|class| item.classes & class != 0)
        && filters
            .iter()
            .zip(&state.stats)
            .filter(|(_, selected)| **selected)
            .all(|(filter, _)| {
                filter.categories.iter().any(|wanted| {
                    item.categories
                        .iter()
                        .any(|category| category_matches(wanted, category))
                })
            })
}

fn category_matches(wanted: &str, category: &str) -> bool {
    wanted.eq_ignore_ascii_case(category) || wanted == format!("{{{:08x}}}", fnv1a(category))
}

fn fnv1a(text: &str) -> u32 {
    text.to_lowercase().bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn recipe_tree(data: &GameData, root: u32, inventory: &[Option<u32>], width: f32) -> Vec<Branch> {
    let mut branches = vec![Branch {
        item: root,
        depth: 0,
        parent: None,
        span: [0.0, width],
        owned: false,
    }];
    let mut consumed = Vec::new();
    let mut index = 0;
    while index < branches.len() {
        let branch = branches[index];
        let components = data
            .item(branch.item)
            .map_or(&[][..], |item| &item.from[..]);
        let share = (branch.span[1] - branch.span[0]) / components.len().max(1) as f32;
        for (offset, component) in components.iter().enumerate() {
            let owned = branch.owned
                || inventory
                    .iter()
                    .enumerate()
                    .position(|(slot, owned)| {
                        *owned == Some(*component) && !consumed.contains(&slot)
                    })
                    .map(|slot| consumed.push(slot))
                    .is_some();
            let start = branch.span[0] + share * offset as f32;
            branches.push(Branch {
                item: *component,
                depth: branch.depth + 1,
                parent: Some(index),
                span: [start, start + share],
                owned,
            });
        }
        index += 1;
    }
    branches
}

fn purchase_label(data: &GameData, root: u32, painted: &Painted) -> &'static str {
    let Some(item) = data.item(root) else {
        return "shop_autobuild_unpurchasable";
    };
    if locked(data, root, painted) {
        return "shop_autobuild_unpurchasable";
    }
    let cost = data
        .purchase(root, &painted.items)
        .map_or(u32::MAX, |purchase| purchase.cost);
    let affordable = painted.gold >= cost;
    if item.from.is_empty() {
        return match (affordable, painted.can_shop) {
            (true, true) => "shop_autobuild_can_purchase",
            (false, _) => "shop_autobuild_trivial",
            (true, false) => "shop_autobuild_out_of_range",
        };
    }
    if !painted.can_shop {
        return "shop_autobuild_want_to_build";
    }
    if affordable {
        "shop_autobuild_can_purchase"
    } else if !component_orders(data, root, &painted.items, painted.gold).is_empty() {
        "shop_autobuild_can_purchase_components"
    } else {
        "shop_autobuild_want_to_build"
    }
}

fn locked(data: &GameData, item: u32, painted: &Painted) -> bool {
    data.stack_slot(item, &painted.items, &painted.stacks)
        .is_none()
        && data.locked(item, &painted.items)
}

fn orders(data: &GameData, root: u32, you: &You) -> Vec<u32> {
    let stacking = data.stack_slot(root, &you.items, &you.stacks).is_some();
    if !you.can_shop || (!stacking && data.locked(root, &you.items)) {
        return Vec::new();
    }
    let gold = you.gold.max(0.0) as u32;
    match data.purchase(root, &you.items) {
        Some(purchase) if purchase.cost <= gold => vec![root],
        _ => component_orders(data, root, &you.items, gold),
    }
}

fn component_orders(data: &GameData, root: u32, items: &[Option<u32>; 6], gold: u32) -> Vec<u32> {
    let tree = recipe_tree(data, root, items, 1.0);
    let mut candidates: Vec<usize> = (1..tree.len())
        .filter(|index| !tree[*index].owned)
        .collect();
    candidates.sort_by_key(|index| {
        std::cmp::Reverse(
            data.item(tree[*index].item)
                .map_or(0, |item| item.total_price),
        )
    });
    let mut inventory = *items;
    let mut gold = gold;
    let mut bought: Vec<usize> = Vec::new();
    let mut orders = Vec::new();
    for index in candidates {
        let mut ancestor = tree[index].parent;
        let mut covered = false;
        while let Some(parent) = ancestor {
            covered |= bought.contains(&parent);
            ancestor = tree[parent].parent;
        }
        if covered {
            continue;
        }
        let item = tree[index].item;
        let Some(purchase) = data.purchase(item, &inventory) else {
            continue;
        };
        if purchase.cost > gold || data.locked(item, &inventory) {
            continue;
        }
        for slot in &purchase.consumed {
            inventory[*slot] = None;
        }
        let Some(slot) = inventory.iter().position(Option::is_none) else {
            continue;
        };
        inventory[slot] = Some(item);
        gold -= purchase.cost;
        bought.push(index);
        orders.push(item);
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset::data::{Item, Limit};

    fn item(id: u32, price: u32, total_price: u32, from: &[u32]) -> Item {
        Item {
            id,
            price,
            total_price,
            from: from.to_vec(),
            ..Item::default()
        }
    }

    fn data() -> GameData {
        GameData {
            items: vec![
                item(1052, 400, 400, &[]),
                item(1026, 850, 850, &[]),
                item(3145, 300, 1100, &[1052, 1052]),
                Item {
                    limits: vec![Limit {
                        group: "shadowflame".into(),
                        max: 1,
                    }],
                    ..item(4645, 1250, 3200, &[3145, 1026])
                },
            ],
            ..GameData::default()
        }
    }

    fn painted(gold: u32, items: [Option<u32>; 6], can_shop: bool) -> Painted {
        Painted {
            state: State::default(),
            gold,
            items,
            stacks: [1; 6],
            can_shop,
            can_undo: false,
        }
    }

    #[test]
    fn hashed_filter_categories_match_item_categories() {
        assert!(category_matches("{00758e29}", "MagicPenetration"));
        assert!(category_matches("{9b2c9850}", "NonbootsMovement"));
        assert!(category_matches("criticalStrike", "CriticalStrike"));
        assert!(!category_matches("Mana", "ManaRegen"));
    }

    #[test]
    fn filters_need_the_class_and_every_selected_stat() {
        let filters = vec![
            StatFilter {
                button: "AP".into(),
                categories: vec!["SpellDamage".into()],
            },
            StatFilter {
                button: "Pen".into(),
                categories: vec!["{00758e29}".into()],
            },
        ];
        let staff = Item {
            classes: MAGE,
            categories: vec!["SpellDamage".into(), "MagicPenetration".into()],
            ..Item::default()
        };
        let tome = Item {
            classes: MAGE | SUPPORT,
            categories: vec!["SpellDamage".into()],
            ..Item::default()
        };
        let state = State {
            class: Some(MAGE),
            stats: vec![true, true],
            ..State::default()
        };
        assert!(matches(&staff, &state, &filters));
        assert!(!matches(&tome, &state, &filters));
        let state = State {
            class: Some(TANK),
            stats: vec![false, false],
            ..State::default()
        };
        assert!(!matches(&staff, &state, &filters));
    }

    #[test]
    fn recipe_trees_split_their_width_and_mark_owned_parts() {
        let tree = recipe_tree(&data(), 4645, &[Some(1052), None], 400.0);
        let summary: Vec<(u32, usize, [f32; 2], bool)> = tree
            .iter()
            .map(|branch| (branch.item, branch.depth, branch.span, branch.owned))
            .collect();
        assert_eq!(
            summary,
            [
                (4645, 0, [0.0, 400.0], false),
                (3145, 1, [0.0, 200.0], false),
                (1026, 1, [200.0, 400.0], false),
                (1052, 2, [0.0, 100.0], true),
                (1052, 2, [100.0, 200.0], false),
            ]
        );
    }

    #[test]
    fn the_purchase_button_follows_shop_wording() {
        let data = data();
        let empty = [None; 6];
        assert_eq!(
            purchase_label(&data, 4645, &painted(5000, empty, true)),
            "shop_autobuild_can_purchase"
        );
        assert_eq!(
            purchase_label(&data, 4645, &painted(900, empty, true)),
            "shop_autobuild_can_purchase_components"
        );
        assert_eq!(
            purchase_label(&data, 4645, &painted(100, empty, true)),
            "shop_autobuild_want_to_build"
        );
        assert_eq!(
            purchase_label(&data, 4645, &painted(5000, empty, false)),
            "shop_autobuild_want_to_build"
        );
        assert_eq!(
            purchase_label(&data, 1052, &painted(100, empty, true)),
            "shop_autobuild_trivial"
        );
        assert_eq!(
            purchase_label(&data, 1052, &painted(500, empty, false)),
            "shop_autobuild_out_of_range"
        );
        let mut owned = empty;
        owned[0] = Some(4645);
        assert_eq!(
            purchase_label(&data, 4645, &painted(5000, owned, true)),
            "shop_autobuild_unpurchasable"
        );
    }

    #[test]
    fn components_are_bought_most_expensive_first() {
        assert_eq!(component_orders(&data(), 4645, &[None; 6], 1300), [3145]);
        assert_eq!(component_orders(&data(), 4645, &[None; 6], 1000), [1026]);
        assert_eq!(component_orders(&data(), 4645, &[None; 6], 500), [1052]);
    }
}
