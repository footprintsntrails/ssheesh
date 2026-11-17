//! macOS: notifications through `UNUserNotificationCenter`, and the menu bar item.
//!
//! **Notifications need a bundle.** `UNUserNotificationCenter` works only for a program inside
//! an `.app` bundle with a bundle identifier (`SSHeesh Preview.app`); asked from a bare binary
//! (`cargo run`, the binary copied out), it raises an Objective-C exception and the app ends.
//! So a bare binary shows no notification: [`notifier`] gives it [`Bounce`], which bounces the
//! Dock icon once instead, and the tab still says "Needs you". Inside a bundle the first
//! notification asks for permission (macOS's own question); a click on one activates the app
//! and reaches [`Delegate`], which hands the tab to the window. Nothing here has run on a Mac
//! yet: it builds for macOS, and wants checking in a bundle there.
//!
//! **The menu bar item** ([`status_item`]) is the face of the logo as a template image, which
//! the menu bar tints, and the tray's menu. It must be made on the main thread (Rune's UI
//! thread is the main thread on macOS).

use std::cell::RefCell;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSImage, NSMenu, NSMenuItem, NSRequestUserAttentionType, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSArray, NSBundle, NSData, NSError, NSSize, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

use crate::tray::{self, Item, TrayHost};
use crate::{escape, AppInfo, Event, Note, Notifier, Sink, Support};

const TITLE_MAX: usize = 120;
const BODY_MAX: usize = 400;

/// Whether this program runs from an `.app` bundle with an identifier.
pub fn bundled() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

/// The notifier this program can have: the notification centre in a bundle, else a bounce.
pub(crate) fn notifier(app: &AppInfo, sink: Sink) -> Box<dyn Notifier> {
    let _ = app;
    if bundled() {
        Box::new(Center::start(sink))
    } else {
        Box::new(Bounce)
    }
}

/// A notification's identifier: its tab, so a newer one for the tab replaces it.
fn identifier(tab: u64) -> String {
    format!("tab-{tab}")
}

fn tab_of(identifier: &str) -> Option<u64> {
    identifier.strip_prefix("tab-").filter(|n| n.bytes().all(|b| b.is_ascii_digit())).and_then(|n| n.parse().ok())
}

/// Hears a notification's click.
pub struct DelegateIvars {
    sink: Sink,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SSHeeshNotificationDelegate"]
    #[ivars = DelegateIvars]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &DynBlock<dyn Fn()>) {
            let id = response.notification().request().identifier().to_string();
            if let Some(tab) = tab_of(&id) {
                (self.ivars().sink)(Event::OpenTab { tab, token: None });
            }
            done.call(());
        }

        /// A notification that arrives while the app is in front still shows (it was sent while
        /// it wasn't).
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(&self, _center: &UNUserNotificationCenter, _notification: &UNNotification, done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
            done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }
    }
);

impl Delegate {
    fn new(sink: Sink) -> Retained<Delegate> {
        let this = Self::alloc().set_ivars(DelegateIvars { sink });
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// `UNUserNotificationCenter`, in a bundle.
struct Center {
    center: Retained<UNUserNotificationCenter>,
    /// The centre holds its delegate weakly.
    _delegate: Retained<Delegate>,
}

impl Center {
    fn start(sink: Sink) -> Center {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new(sink);
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // macOS asks once; the answer is kept in System Settings → Notifications.
        let asked = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &asked);
        Center { center, _delegate: delegate }
    }
}

impl Notifier for Center {
    fn show(&mut self, note: &Note) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&escape::clip(&note.title, TITLE_MAX)));
        content.setBody(&NSString::from_str(&escape::clip(&note.body, BODY_MAX)));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&identifier(note.tab)), &content, None);
        self.center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    fn withdraw(&mut self, tab: u64) {
        let ids = NSArray::from_retained_slice(&[NSString::from_str(&identifier(tab))]);
        self.center.removeDeliveredNotificationsWithIdentifiers(&ids);
    }

    fn support(&self) -> Support {
        Support::Full
    }
}

/// A bare binary: the Dock icon bounces once, as a terminal app's does on a bell.
pub struct Bounce;

impl Notifier for Bounce {
    fn show(&mut self, _: &Note) {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
        }
    }

    fn withdraw(&mut self, _: u64) {}

    fn support(&self) -> Support {
        Support::None("macOS shows notifications only for an app in a bundle (SSHeesh Preview.app); run on its own, the Dock icon bounces instead.")
    }
}

/// The menu's items' ids, by their tags, and where picks go.
pub struct TargetIvars {
    sink: Sink,
    ids: RefCell<Vec<String>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SSHeeshMenuTarget"]
    #[ivars = TargetIvars]
    pub struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &NSMenuItem) {
            let id = usize::try_from(sender.tag()).ok().and_then(|i| self.ivars().ids.borrow().get(i).cloned());
            if let Some(event) = id.and_then(|id| tray::route(&id, None)) {
                (self.ivars().sink)(event);
            }
        }
    }
);

impl MenuTarget {
    fn new(sink: Sink, mtm: MainThreadMarker) -> Retained<MenuTarget> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { sink, ids: RefCell::new(Vec::new()) });
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// The menu bar item.
pub struct StatusItem {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<MenuTarget>,
    mtm: MainThreadMarker,
}

/// Puts the item in the menu bar; picks from its menu go to `sink`. `None` off the main thread.
pub fn status_item(app: &AppInfo, sink: Sink) -> Option<StatusItem> {
    let mtm = MainThreadMarker::new()?;
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let (w, h) = (64, 36);
    let png = tray::png_of(w, h, &tray::template(w, h));
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))?;
    image.setTemplate(true);
    // Two pixels a point: 32 by 18 points, the menu bar's height less its margins.
    image.setSize(NSSize::new(w as f64 / 2.0, h as f64 / 2.0));
    if let Some(button) = item.button(mtm) {
        button.setImage(Some(&image));
        button.setToolTip(Some(&NSString::from_str(&app.name)));
    }
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    item.setMenu(Some(&menu));
    Some(StatusItem { item, menu, target: MenuTarget::new(sink, mtm), mtm })
}

impl TrayHost for StatusItem {
    fn set_menu(&self, items: Vec<Item>) {
        self.menu.removeAllItems();
        let mut ids = Vec::new();
        for item in items {
            match item {
                Item::Separator => self.menu.addItem(&NSMenuItem::separatorItem(self.mtm)),
                Item::Action { id, label, enabled } => {
                    // SAFETY: `pick:` is MenuTarget's, which is the item's target.
                    let entry = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &NSString::from_str(&label), Some(sel!(pick:)), &NSString::from_str("")) };
                    // SAFETY: the target lives as long as this item (both are ours).
                    unsafe { entry.setTarget(Some(&self.target)) };
                    entry.setTag(ids.len() as isize);
                    entry.setEnabled(enabled);
                    ids.push(id);
                    self.menu.addItem(&entry);
                }
            }
        }
        *self.target.ivars().ids.borrow_mut() = ids;
    }

    fn set_tooltip(&self, tooltip: String) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setToolTip(Some(&NSString::from_str(&tooltip)));
        }
    }
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_name_their_tab() {
        assert_eq!(tab_of(&identifier(42)), Some(42));
        assert_eq!(tab_of("tab-"), None);
        assert_eq!(tab_of("tab-4x"), None);
        assert_eq!(tab_of("other"), None);
    }
}
