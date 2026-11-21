//! The floating widget window: a borderless, non-activating `NSPanel` that floats above
//! other apps on every Space, with a custom view that draws the widget and forwards
//! mouse input to `app`.

use crate::app;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSMenuItem, NSPanel, NSView, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize};

/// `kCGFloatingWindowLevel`: above normal windows, like a utility palette.
const FLOATING_WINDOW_LEVEL: isize = 3;

define_class!(
    // The widget's content view. All state lives in `app`; these methods only translate
    // AppKit callbacks into calls there.
    #[unsafe(super(NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CUWWidgetView"]
    pub struct WidgetView;

    unsafe impl NSObjectProtocol for WidgetView {}

    impl WidgetView {
        /// Top-left origin, like Windows, so the shared layout numbers apply directly.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The panel never becomes active; accept the click that would otherwise only
        /// activate the window.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            app::draw(self.bounds().size.height);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            app::on_mouse_down(self, event, p);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            app::on_right_mouse_down(self, event);
        }

        /// Target/action of every context-menu item; the item's tag is its command id.
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, sender: &NSMenuItem) {
            app::on_menu_command(sender.tag() as usize);
        }
    }
);

define_class!(
    // Window delegate of the panel: reports moves so the position can be saved.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CUWPanelDelegate"]
    pub struct PanelDelegate;

    unsafe impl NSObjectProtocol for PanelDelegate {}

    unsafe impl NSWindowDelegate for PanelDelegate {
        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            app::on_window_moved();
        }
    }
);

impl WidgetView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<WidgetView> {
        unsafe { msg_send![WidgetView::alloc(mtm), initWithFrame: frame] }
    }
}

/// Create the (still hidden) widget panel of `width × height` points, its view, and its
/// delegate (which the caller must keep alive).
pub fn create(
    mtm: MainThreadMarker,
    width: f64,
    height: f64,
) -> (Retained<NSPanel>, Retained<WidgetView>, Retained<PanelDelegate>) {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel: Retained<NSPanel> = unsafe {
        msg_send![
            NSPanel::alloc(mtm),
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    unsafe {
        // We own the panel through `Retained`; AppKit must not also release it on close.
        panel.setReleasedWhenClosed(false);
        panel.setFloatingPanel(true);
        panel.setLevel(FLOATING_WINDOW_LEVEL);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        // Transparent window; the view paints a rounded background.
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(true);
        // Dragging is started explicitly in `app::on_mouse_down` so clicks on ✕ and
        // "click to sign in" still work.
        panel.setMovableByWindowBackground(false);
    }
    let view = WidgetView::new(mtm, frame);
    panel.setContentView(Some(&view));
    let delegate: Retained<PanelDelegate> = unsafe { msg_send![PanelDelegate::alloc(mtm), init] };
    panel.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    (panel, view, delegate)
}
