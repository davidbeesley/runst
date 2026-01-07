use crate::config::{Config, GlobalConfig, Origin};
use crate::error::{Error, Result};
use crate::notification::{Manager, NOTIFICATION_MESSAGE_TEMPLATE, Notification};
use cairo::{
    Context as CairoContext, XCBConnection as CairoXCBConnection, XCBDrawable, XCBSurface,
    XCBVisualType,
};
use colorsys::ColorAlpha;
use pango::{Context as PangoContext, FontDescription, Layout as PangoLayout};
use pangocairo::functions as pango_functions;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;
use tera::{Result as TeraResult, Tera, Value};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::{Event, xproto::*};
use x11rb::xcb_ffi::XCBConnection;

/// Rust version of XCB's [`xcb_visualtype_t`] struct.
///
/// [`xcb_visualtype_t`]: https://xcb.freedesktop.org/manual/structxcb__visualtype__t.html
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct xcb_visualtype_t {
    visual_id: u32,
    class: u8,
    bits_per_rgb_value: u8,
    colormap_entries: u16,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    pad0: [u8; 4],
}

impl From<Visualtype> for xcb_visualtype_t {
    fn from(value: Visualtype) -> xcb_visualtype_t {
        xcb_visualtype_t {
            visual_id: value.visual_id,
            class: value.class.into(),
            bits_per_rgb_value: value.bits_per_rgb_value,
            colormap_entries: value.colormap_entries,
            red_mask: value.red_mask,
            green_mask: value.green_mask,
            blue_mask: value.blue_mask,
            pad0: [0; 4],
        }
    }
}

/// Wrapper for X11 [`connection`] and [`screen`].
///
/// [`connection`]: XCBConnection
/// [`screen`]: x11rb::protocol::xproto::Screen
pub struct X11 {
    connection: XCBConnection,
    cairo: CairoXCBConnection,
    screen: Screen,
}

unsafe impl Send for X11 {}
unsafe impl Sync for X11 {}

/// Calculates window position based on origin anchor point.
fn calculate_position_from_origin(
    origin: Origin,
    offset_x: u32,
    offset_y: u32,
    width: u32,
    height: u32,
    screen_width: u16,
    screen_height: u16,
) -> (i16, i16) {
    let screen_w = screen_width as i32;
    let screen_h = screen_height as i32;
    let off_x = offset_x as i32;
    let off_y = offset_y as i32;
    let w = width as i32;
    let h = height as i32;

    let (x, y) = match origin {
        Origin::TopLeft => (off_x, off_y),
        Origin::TopRight => (screen_w - w - off_x, off_y),
        Origin::BottomLeft => (off_x, screen_h - h - off_y),
        Origin::BottomRight => (screen_w - w - off_x, screen_h - h - off_y),
    };

    (x.max(0) as i16, y.max(0) as i16)
}

impl X11 {
    /// Initializes the X11 connection.
    pub fn init(screen_num: Option<usize>) -> Result<Self> {
        let (connection, default_screen_num) = XCBConnection::connect(None)?;
        log::trace!("Default screen num: {:?}", default_screen_num);
        let setup_info = connection.setup();
        log::trace!("Setup info status: {:?}", setup_info.status);
        let screen = setup_info.roots[screen_num.unwrap_or(default_screen_num)].clone();
        log::trace!("Screen root: {:?}", screen.root);
        let cairo =
            unsafe { CairoXCBConnection::from_raw_none(connection.get_raw_xcb_connection() as _) };
        Ok(Self {
            connection,
            screen,
            cairo,
        })
    }

    /// Creates a window.
    pub fn create_window(&mut self, config: &GlobalConfig) -> Result<X11Window> {
        let visual_id = self.screen.root_visual;
        let mut visual_type = self
            .find_xcb_visualtype(visual_id)
            .ok_or_else(|| Error::X11Other(String::from("cannot find a XCB visual type")))?;
        let visual = unsafe { XCBVisualType::from_raw_none(&mut visual_type as *mut _ as _) };
        let window_id = self.connection.generate_id()?;
        log::trace!("Window ID: {:?}", window_id);

        let screen_width = self.screen.width_in_pixels;
        let screen_height = self.screen.height_in_pixels;
        let initial_width = config.geometry.width;
        let initial_height = config.geometry.height;

        // Calculate initial position based on origin
        // geometry.x and geometry.y are treated as offsets from the origin
        let (x, y) = calculate_position_from_origin(
            config.origin,
            config.geometry.x,
            config.geometry.y,
            initial_width,
            initial_height,
            screen_width,
            screen_height,
        );

        log::debug!(
            "Creating window at ({}, {}) size {}x{} origin={} screen={}x{}",
            x,
            y,
            initial_width,
            initial_height,
            config.origin,
            screen_width,
            screen_height
        );

        self.connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            window_id,
            self.screen.root,
            x,
            y,
            initial_width.try_into()?,
            initial_height.try_into()?,
            0,
            WindowClass::INPUT_OUTPUT,
            visual_id,
            &CreateWindowAux::new()
                .border_pixel(self.screen.white_pixel)
                .override_redirect(1)
                .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS),
        )?;
        let surface = XCBSurface::create(
            &self.cairo,
            &XCBDrawable(window_id),
            &visual,
            config.geometry.width.try_into()?,
            config.geometry.height.try_into()?,
        )?;
        let context = CairoContext::new(&surface)?;
        X11Window::new(
            window_id,
            surface,
            context,
            &config.font,
            Box::leak(config.template.to_string().into_boxed_str()),
            config.origin,
            config.geometry.x,
            config.geometry.y,
            screen_width,
            screen_height,
        )
    }

    /// Find a `xcb_visualtype_t` based on its ID number
    fn find_xcb_visualtype(&self, visual_id: u32) -> Option<xcb_visualtype_t> {
        for root in &self.connection.setup().roots {
            for depth in &root.allowed_depths {
                for visual in &depth.visuals {
                    if visual.visual_id == visual_id {
                        return Some((*visual).into());
                    }
                }
            }
        }
        None
    }

    /// Shows the given X11 window.
    pub fn show_window(&self, window: &X11Window) -> Result<()> {
        window.show(&self.connection)?;
        self.connection.flush()?;
        Ok(())
    }

    /// Hides the given X11 window.
    pub fn hide_window(&self, window: &X11Window) -> Result<()> {
        window.hide(&self.connection)?;
        self.connection.flush()?;
        Ok(())
    }

    /// Width of the close button area on the right side of each notification.
    const CLOSE_BUTTON_WIDTH: i32 = 30;

    /// Handles X11 events in a loop, calling `on_press` when a notification is clicked.
    /// The callback receives (notifications, clicked_index, invoke_action) where
    /// invoke_action is false if the close button was clicked.
    pub fn handle_events<F>(
        &self,
        window: Arc<X11Window>,
        manager: Manager,
        config: Arc<Config>,
        on_press: F,
    ) -> Result<()>
    where
        F: Fn(Vec<Notification>, Option<usize>, bool), // (notifications, clicked_idx, invoke_action)
    {
        let display_limit = config.global.display_limit;
        let refresh_interval = config.global.refresh_interval_ms;

        // Use short poll interval for responsiveness, track time for redraws
        const POLL_INTERVAL_MS: u64 = 50;
        let mut last_redraw = std::time::Instant::now();

        loop {
            self.connection.flush()?;

            // If refresh is enabled and there are unread notifications, use polling with timeout
            // Otherwise, block waiting for events
            let has_unread = manager.get_unread_count() > 0;
            let use_refresh = refresh_interval > 0 && has_unread;

            if use_refresh {
                // Non-blocking poll for events
                let mut event_opt = self.connection.poll_for_event()?;

                if event_opt.is_none() {
                    // No events, short sleep for responsiveness
                    std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

                    // Only redraw at refresh_interval rate
                    if last_redraw.elapsed().as_millis() >= refresh_interval as u128 {
                        let notifications = manager.get_unread_buffer(display_limit);
                        let unread_count = manager.get_unread_count();
                        if !notifications.is_empty() {
                            window.draw(&self.connection, notifications, unread_count, &config)?;
                        }
                        last_redraw = std::time::Instant::now();
                    }
                    continue;
                }

                // Process any pending events
                while let Some(event) = event_opt {
                    log::trace!("New event: {:?}", event);
                    match event {
                        Event::Expose(_) => {
                            let notifications = manager.get_unread_buffer(display_limit);
                            let unread_count = manager.get_unread_count();
                            window.draw(&self.connection, notifications, unread_count, &config)?;
                        }
                        Event::ButtonPress(ev) => {
                            let unread = manager.get_unread_buffer(display_limit);
                            let clicked_idx = window.get_clicked_index(ev.event_y as i32);
                            let window_width = window.get_window_width();
                            let invoke_action = (ev.event_x as i32) < window_width - Self::CLOSE_BUTTON_WIDTH;
                            // Don't mark all as read here - let callback handle individual closes
                            on_press(unread, clicked_idx, invoke_action);
                        }
                        _ => {}
                    }
                    event_opt = self.connection.poll_for_event()?;
                }
            } else {
                // Block waiting for events (original behavior)
                let event = self.connection.wait_for_event()?;
                let mut event_opt = Some(event);
                while let Some(event) = event_opt {
                    log::trace!("New event: {:?}", event);
                    match event {
                        Event::Expose(_) => {
                            let notifications = manager.get_unread_buffer(display_limit);
                            let unread_count = manager.get_unread_count();
                            window.draw(&self.connection, notifications, unread_count, &config)?;
                        }
                        Event::ButtonPress(ev) => {
                            let unread = manager.get_unread_buffer(display_limit);
                            let clicked_idx = window.get_clicked_index(ev.event_y as i32);
                            let window_width = window.get_window_width();
                            let invoke_action = (ev.event_x as i32) < window_width - Self::CLOSE_BUTTON_WIDTH;
                            // Don't mark all as read here - let callback handle individual closes
                            on_press(unread, clicked_idx, invoke_action);
                        }
                        _ => {}
                    }
                    event_opt = self.connection.poll_for_event()?;
                }
            }
        }
    }
}

/// Representation of a X11 window.
pub struct X11Window {
    /// Window ID.
    pub id: u32,
    /// Cairo surface for drawing.
    pub surface: XCBSurface,
    /// Graphics renderer context.
    pub cairo_context: CairoContext,
    /// Text renderer context.
    pub pango_context: PangoContext,
    /// Window layout.
    pub layout: PangoLayout,
    /// Text format.
    pub template: Tera,
    /// Window origin/anchor point.
    pub origin: Origin,
    /// X offset from origin.
    pub offset_x: u32,
    /// Y offset from origin.
    pub offset_y: u32,
    /// Screen width in pixels.
    pub screen_width: u16,
    /// Screen height in pixels.
    pub screen_height: u16,
    /// Entry bounds for click detection: (y_start, y_end, index in original notifications vec)
    pub entry_bounds: std::sync::Mutex<Vec<(i32, i32, usize)>>,
    /// Current window width (updated during draw)
    pub current_width: std::sync::Mutex<i32>,
}

unsafe impl Send for X11Window {}
unsafe impl Sync for X11Window {}

impl X11Window {
    /// Creates a new instance of window.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        surface: XCBSurface,
        cairo_context: CairoContext,
        font: &str,
        raw_template: &'static str,
        origin: Origin,
        offset_x: u32,
        offset_y: u32,
        screen_width: u16,
        screen_height: u16,
    ) -> Result<Self> {
        let pango_context = pango_functions::create_context(&cairo_context);
        let layout = PangoLayout::new(&pango_context);
        let font_description = FontDescription::from_string(font);
        pango_context.set_font_description(Some(&font_description));
        let mut template = Tera::default();
        if let Err(e) =
            template.add_raw_template(NOTIFICATION_MESSAGE_TEMPLATE, raw_template.trim())
        {
            return if let Some(error_source) = e.source() {
                Err(Error::TemplateParse(error_source.to_string()))
            } else {
                Err(Error::Template(e))
            };
        }
        template.register_filter(
            "humantime",
            |value: &Value, _: &HashMap<String, Value>| -> TeraResult<Value> {
                let value = tera::try_get_value!("humantime_filter", "value", u64, value);
                let value = humantime::format_duration(Duration::new(value, 0)).to_string();
                Ok(tera::to_value(value)?)
            },
        );
        Ok(Self {
            id,
            surface,
            cairo_context,
            pango_context,
            layout,
            template,
            origin,
            offset_x,
            offset_y,
            screen_width,
            screen_height,
            entry_bounds: std::sync::Mutex::new(Vec::new()),
            current_width: std::sync::Mutex::new(0),
        })
    }

    /// Returns the index of the clicked notification based on y coordinate.
    /// Returns None if click was on a separator or outside notification bounds.
    pub fn get_clicked_index(&self, y: i32) -> Option<usize> {
        if let Ok(bounds) = self.entry_bounds.lock() {
            for (y_start, y_end, idx) in bounds.iter() {
                if y >= *y_start && y < *y_end {
                    return Some(*idx);
                }
            }
        }
        None
    }

    /// Returns the current window width.
    pub fn get_window_width(&self) -> i32 {
        self.current_width.lock().map(|w| *w).unwrap_or(0)
    }

    /// Calculates the X,Y position based on origin, offsets, and window size.
    pub fn calculate_position(&self, width: u32, height: u32) -> (i32, i32) {
        let screen_w = self.screen_width as i32;
        let screen_h = self.screen_height as i32;
        let offset_x = self.offset_x as i32;
        let offset_y = self.offset_y as i32;
        let w = width as i32;
        let h = height as i32;

        match self.origin {
            Origin::TopLeft => (offset_x, offset_y),
            Origin::TopRight => (screen_w - w - offset_x, offset_y),
            Origin::BottomLeft => (offset_x, screen_h - h - offset_y),
            Origin::BottomRight => (screen_w - w - offset_x, screen_h - h - offset_y),
        }
    }

    /// Shows the window.
    fn show(&self, connection: &impl Connection) -> Result<()> {
        connection.map_window(self.id)?;
        Ok(())
    }

    /// Hides the window.
    fn hide(&self, connection: &impl Connection) -> Result<()> {
        connection.unmap_window(self.id)?;
        Ok(())
    }

    /// Escapes text for safe inclusion in Pango markup.
    fn escape_markup(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    /// Draws the window content with multiple notifications.
    fn draw(
        &self,
        connection: &XCBConnection,
        notifications: Vec<Notification>,
        unread_count: usize,
        config: &Config,
    ) -> Result<()> {
        if notifications.is_empty() {
            return Ok(());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Set layout width for text wrapping
        let wrap_width = config.global.min_width.unwrap_or(600) as i32;
        self.layout.set_width(wrap_width * pango::SCALE);
        self.layout.set_wrap(pango::WrapMode::WordChar);

        // Reverse to show newest first
        let mut notifications_reversed: Vec<_> = notifications.iter().collect();
        notifications_reversed.reverse();

        // Build notification entries with their markup and background colors
        struct NotificationEntry {
            markup: String,
            bg_color: Option<String>,
            height: i32,
            is_separator: bool,
            /// Index in original notifications vec (None for separators and footer)
            original_index: Option<usize>,
        }

        let separator_height = 2; // pixels
        let mut entries: Vec<NotificationEntry> = Vec::new();

        for (idx, notification) in notifications_reversed.iter().enumerate() {
            let urgency_config = config.get_urgency_config(&notification.urgency);
            urgency_config.run_commands(notification)?;

            // Calculate age in seconds
            let age_secs = now.saturating_sub(notification.timestamp);

            // Check for matching rule first, then app_colors, then default
            let matching_rule = config.get_matching_rule(
                &notification.app_name,
                &notification.summary,
                &notification.body,
            );

            // Get background color from rule or app_colors
            let bg_color = matching_rule
                .and_then(|r| r.background.as_ref())
                .or_else(|| config.get_app_color(&notification.app_name))
                .cloned();

            // Format age display
            let age_display = if age_secs < 60 {
                format!("{:>3}s", age_secs)
            } else if age_secs < 3600 {
                format!("{:>3}m", age_secs / 60)
            } else {
                format!("{:>3}h", age_secs / 3600)
            };

            // Escape text for Pango markup (preserve newlines in body)
            let app_name_escaped = Self::escape_markup(&notification.app_name);
            let summary_escaped = Self::escape_markup(&notification.summary);
            let body_escaped = Self::escape_markup(&notification.body);

            // Build the notification line with Pango markup (no background attr)
            let markup = format!(
                "<tt><span foreground=\"#888888\">{}</span></tt> {} <b>{}</b>{}",
                age_display,
                app_name_escaped,
                summary_escaped,
                if notification.body.is_empty() {
                    String::new()
                } else {
                    format!("\n  {}", body_escaped)
                }
            );

            // Calculate height for this entry
            self.layout.set_markup(&markup);
            let (_, height) = self.layout.pixel_size();

            // Map reversed index back to original: notifications_reversed[idx] == notifications[len-1-idx]
            let original_idx = notifications.len() - 1 - idx;

            entries.push(NotificationEntry {
                markup,
                bg_color,
                height,
                is_separator: false,
                original_index: Some(original_idx),
            });

            // Add separator between notifications (but not after the last one)
            if idx < notifications_reversed.len() - 1 {
                entries.push(NotificationEntry {
                    markup: String::new(),
                    bg_color: None,
                    height: separator_height,
                    is_separator: true,
                    original_index: None,
                });
            }
        }

        // Add unread count if more than displayed
        if unread_count > notifications.len() {
            let more_markup = format!(
                "<span foreground=\"#888888\"><i>... and {} more</i></span>",
                unread_count - notifications.len()
            );
            self.layout.set_markup(&more_markup);
            let (_, height) = self.layout.pixel_size();
            entries.push(NotificationEntry {
                markup: more_markup,
                bg_color: None,
                height,
                is_separator: false,
                original_index: None,
            });
        }

        // Calculate total height
        let total_height: i32 = entries.iter().map(|e| e.height).sum();

        // Use the urgency of the most recent notification for default background color
        let newest_notification = notifications_reversed
            .first()
            .expect("notifications not empty");
        let urgency_config = config.get_urgency_config(&newest_notification.urgency);

        // Calculate window dimensions
        let width_u32 = wrap_width as u32;
        let height_u32 = total_height.max(1) as u32;

        // Store current width for click detection
        if let Ok(mut w) = self.current_width.lock() {
            *w = wrap_width;
        }

        // Calculate and apply window size if wrap_content is enabled
        if config.global.wrap_content {
            // Calculate new position based on origin and new size
            let (x, y) = calculate_position_from_origin(
                self.origin,
                self.offset_x,
                self.offset_y,
                width_u32,
                height_u32,
                self.screen_width,
                self.screen_height,
            );

            // Resize and reposition the window
            let values = ConfigureWindowAux::default()
                .x(Some(x.into()))
                .y(Some(y.into()))
                .width(Some(width_u32))
                .height(Some(height_u32));
            connection.configure_window(self.id, &values)?;

            // Resize the cairo surface to match the new window size
            self.surface.set_size(width_u32 as i32, height_u32 as i32)?;
        }

        // Clear the entire surface with default background color
        let background_color = urgency_config.background;
        self.cairo_context.set_source_rgba(
            background_color.red() / 255.0,
            background_color.green() / 255.0,
            background_color.blue() / 255.0,
            background_color.alpha(),
        );
        self.cairo_context.paint()?;

        // Draw each entry with its background and text
        let foreground_color = urgency_config.foreground;
        let mut y_pos = 0.0_f64;

        // Clear and rebuild entry bounds for click detection
        let mut new_bounds = Vec::new();

        for entry in &entries {
            let y_start = y_pos as i32;
            let y_end = (y_pos + entry.height as f64) as i32;

            if entry.is_separator {
                // Draw separator as a horizontal line
                self.cairo_context.set_source_rgba(0.27, 0.27, 0.27, 1.0); // #444444
                self.cairo_context
                    .rectangle(0.0, y_pos, width_u32 as f64, entry.height as f64);
                self.cairo_context.fill()?;
            } else {
                // Track bounds for notification entries (not footer)
                if let Some(idx) = entry.original_index {
                    new_bounds.push((y_start, y_end, idx));
                }

                // Draw background rectangle if this entry has a custom color
                if let Some(ref color) = entry.bg_color
                    && let Ok(rgb) = colorsys::Rgb::from_hex_str(color)
                {
                    self.cairo_context.set_source_rgba(
                        rgb.red() / 255.0,
                        rgb.green() / 255.0,
                        rgb.blue() / 255.0,
                        1.0,
                    );
                    self.cairo_context
                        .rectangle(0.0, y_pos, width_u32 as f64, entry.height as f64);
                    self.cairo_context.fill()?;
                }

                // Draw the text
                self.cairo_context.set_source_rgba(
                    foreground_color.red() / 255.0,
                    foreground_color.green() / 255.0,
                    foreground_color.blue() / 255.0,
                    foreground_color.alpha(),
                );
                self.cairo_context.move_to(0., y_pos);
                self.layout.set_markup(&entry.markup);
                pango_functions::show_layout(&self.cairo_context, &self.layout);

                // Draw close button (×) on the right side for notification entries
                if entry.original_index.is_some() {
                    let close_btn_width = 30.0_f64;
                    let close_x = width_u32 as f64 - close_btn_width;
                    let center_y = y_pos + (entry.height as f64 / 2.0);

                    // Draw subtle background for close button
                    self.cairo_context.set_source_rgba(0.3, 0.3, 0.3, 0.5);
                    self.cairo_context
                        .rectangle(close_x, y_pos, close_btn_width, entry.height as f64);
                    self.cairo_context.fill()?;

                    // Draw × symbol
                    self.cairo_context.set_source_rgba(0.7, 0.7, 0.7, 1.0);
                    self.layout.set_markup("<b>×</b>");
                    let (text_w, text_h) = self.layout.pixel_size();
                    self.cairo_context.move_to(
                        close_x + (close_btn_width - text_w as f64) / 2.0,
                        center_y - (text_h as f64 / 2.0),
                    );
                    pango_functions::show_layout(&self.cairo_context, &self.layout);
                }
            }

            y_pos += entry.height as f64;
        }

        // Store bounds for click detection
        if let Ok(mut bounds) = self.entry_bounds.lock() {
            *bounds = new_bounds;
        }

        // Flush the surface to ensure changes are visible
        self.surface.flush();

        Ok(())
    }
}
