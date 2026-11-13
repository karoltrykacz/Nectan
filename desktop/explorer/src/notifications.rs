use crate::{App, NotificationsBridge};
use slint::{ComponentHandle, Image, SharedString, Timer, TimerMode};
use slint::{Global, Weak};
use std::cell::OnceCell;
use std::{
    cell::RefCell, collections::VecDeque, rc::Rc, time::Duration,
};

thread_local! {
    static NOTIFICATIONS: OnceCell<Rc<Notifications>> = const { OnceCell::new() };
}

const SHOW_TIME: Duration = Duration::from_millis(5000);
const SWAP_TIME: Duration = Duration::from_millis(2000);
const SWITCH_TIME: Duration = Duration::from_millis(250);

struct Item {
    icon: Image,
    title: SharedString,
    subtitle: SharedString,
}

struct Notifications {
    w: slint::Weak<App>,
    queue: RefCell<VecDeque<Item>>,
    show_timer: Timer,
    swap_timer: Timer,
    switch_timer: Timer,
}

impl Notifications {
    fn new(w: slint::Weak<App>) -> Rc<Self> {
        Rc::new(Self {
            w,
            queue: RefCell::default(),
            show_timer: Timer::default(),
            swap_timer: Timer::default(),
            switch_timer: Timer::default(),
        })
    }

    fn push(
        self: &Rc<Self>,
        icon: Image,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
    ) {
        let first = {
            let mut q = self.queue.borrow_mut();
            q.push_back(Item {
                icon,
                title: title.into(),
                subtitle: subtitle.into(),
            });
            q.len() == 1
        };
        if first {
            self.show_front();
        } else if let Some(w) = self.w.upgrade() {
            let n = self.queue.borrow().len() as i32;
            let b = NotificationsBridge::get(&w);
            b.set_count(n);
        }
    }

    fn show_front(self: &Rc<Self>) {
        let Some(w) = self.w.upgrade() else { return };
        {
            let q = self.queue.borrow();
            let Some(front) = q.front() else { return };
            let b = NotificationsBridge::get(&w);

            b.set_icon(front.icon.clone());
            b.set_title(front.title.clone());
            b.set_subtitle(front.subtitle.clone());
            b.set_count(q.len() as i32);
            b.set_show_icon(false);
            b.set_switching(false);
            b.set_open(true);
        }

        let this = self.clone();
        self.show_timer.start(
            TimerMode::SingleShot,
            SHOW_TIME,
            move || this.advance(),
        );

        let weak = self.w.clone();
        self.swap_timer.start(
            TimerMode::Repeated,
            SWAP_TIME,
            move || {
                if let Some(w) = weak.upgrade() {
                    let b = NotificationsBridge::get(&w);
                    b.set_show_icon(!b.get_show_icon());
                }
            },
        );
    }

    fn close(&self) {
        self.show_timer.stop();
        self.swap_timer.stop();
        self.switch_timer.stop();
        if let Some(w) = self.w.upgrade() {
            let b = NotificationsBridge::get(&w);
            b.set_count(0);
            b.set_show_icon(false);
            b.set_open(false);
        }
    }

    fn advance(self: &Rc<Self>) {
        self.show_timer.stop();
        self.queue.borrow_mut().pop_front();
        let remaining = self.queue.borrow().len();
        if remaining == 0 {
            self.close();
            return;
        }

        self.swap_timer.stop();
        if let Some(w) = self.w.upgrade() {
            let b = NotificationsBridge::get(&w);
            b.set_count(remaining as i32);
            b.set_show_icon(false);
            b.set_switching(true);
        }
        let this = self.clone();
        self.switch_timer.start(
            TimerMode::SingleShot,
            SWITCH_TIME,
            move || this.show_front(),
        );
    }

    fn dismiss_front(self: &Rc<Self>) {
        if self.switch_timer.running()
            || self.queue.borrow().is_empty()
        {
            return;
        }
        self.advance();
    }
}
pub fn show_notification(
    w: &Weak<App>,
    title: String,
    subtitle: String,
) {
    let _ = w.upgrade_in_event_loop(move |w| {
        NOTIFICATIONS.with(|n| {
            let n = n.get().unwrap();
            n.push(Image::default(), title, subtitle);
        });
    });
}

pub fn init_notifications(w: &App) {
    let n = Notifications::new(w.as_weak());
    NOTIFICATIONS.with(|c| c.set(n.clone()).ok().unwrap());

    let b = NotificationsBridge::get(w);

    b.on_show({
        let n = n.clone();
        move |icon, title, subtitle| n.push(icon, title, subtitle)
    });

    b.on_dismiss({
        let n = n.clone();
        move || n.dismiss_front()
    });
}
