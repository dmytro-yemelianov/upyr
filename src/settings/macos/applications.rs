use super::*;
use crate::application_layout::ApplicationLayoutRule;
use upyr_core::InputLayout;

pub(super) struct ApplicationControls {
    pub page: Retained<NSView>,
    container: Retained<NSView>,
    rows: RefCell<Vec<Row>>,
}

struct Row {
    app: Retained<NSTextField>,
    layout: Retained<NSPopUpButton>,
    remove: Retained<NSButton>,
}

impl ApplicationControls {
    pub fn new(controller: &NativeController, mtm: MainThreadMarker) -> Self {
        let page = page(mtm);
        page_header(&page, "Application layouts", "", mtm);
        page.addSubview(&multiline_label(
            super::super::applications::HELP,
            rect(24.0, 342.0, 624.0, 58.0),
            mtm,
        ));
        let scroll =
            NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(24.0, 110.0, 624.0, 230.0));
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        let container = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 604.0, 230.0));
        scroll.setDocumentView(Some(&container));
        page.addSubview(&scroll);
        page.addSubview(&action_button(
            tr("Add application"),
            rect(24.0, 66.0, 190.0, 30.0),
            controller,
            sel!(addApplicationRule:),
            mtm,
        ));
        page.addSubview(&action_button(
            tr("Raycast → English"),
            rect(224.0, 66.0, 220.0, 30.0),
            controller,
            sel!(addRaycastRule:),
            mtm,
        ));
        page.addSubview(&multiline_label(
            "Use an exact app name, such as Raycast, or its bundle ID. Upyr does not switch layouts for apps without a rule.",
            rect(24.0, 16.0, 624.0, 44.0), mtm,
        ));
        Self {
            page,
            container,
            rows: RefCell::new(Vec::new()),
        }
    }

    pub fn rules(&self) -> Vec<ApplicationLayoutRule> {
        self.rows
            .borrow()
            .iter()
            .map(|r| ApplicationLayoutRule {
                application: r.app.stringValue().to_string(),
                layout: if r.layout.indexOfSelectedItem() == 1 {
                    InputLayout::Ukrainian
                } else {
                    InputLayout::English
                },
            })
            .collect()
    }

    pub fn load(&self, rules: &[ApplicationLayoutRule], controller: &NativeController) {
        for row in self.rows.borrow_mut().drain(..) {
            row.app.removeFromSuperview();
            row.layout.removeFromSuperview();
            row.remove.removeFromSuperview();
        }
        for rule in rules {
            self.add(rule, controller);
        }
    }

    pub fn add(&self, rule: &ApplicationLayoutRule, controller: &NativeController) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if self.rows.borrow().len() >= 128 {
            return;
        }
        let app = text_field(&rule.application, rect(0.0, 0.0, 290.0, 26.0), mtm);
        app.setPlaceholderString(Some(&NSString::from_str(tr(
            super::super::applications::APP_HINT,
        ))));
        let layout = popup(
            &[tr("English"), tr("Ukrainian")],
            rect(0.0, 0.0, 170.0, 28.0),
            mtm,
        );
        layout.selectItemAtIndex(if rule.layout == InputLayout::Ukrainian {
            1
        } else {
            0
        });
        let remove = action_button(
            tr("Remove"),
            rect(0.0, 0.0, 104.0, 28.0),
            controller,
            sel!(removeApplicationRule:),
            mtm,
        );
        self.container.addSubview(&app);
        self.container.addSubview(&layout);
        self.container.addSubview(&remove);
        self.rows.borrow_mut().push(Row {
            app,
            layout,
            remove,
        });
        self.arrange();
    }

    pub fn remove(&self, tag: isize) {
        let Ok(index) = usize::try_from(tag) else {
            return;
        };
        let mut rows = self.rows.borrow_mut();
        if index >= rows.len() {
            return;
        }
        let row = rows.remove(index);
        row.app.removeFromSuperview();
        row.layout.removeFromSuperview();
        row.remove.removeFromSuperview();
        drop(rows);
        self.arrange();
    }

    fn arrange(&self) {
        let rows = self.rows.borrow();
        let height = (rows.len() as f64 * 38.0 + 16.0).max(230.0);
        self.container.setFrameSize(NSSize::new(604.0, height));
        for (i, row) in rows.iter().enumerate() {
            let y = height - 36.0 - i as f64 * 38.0;
            row.app.setFrame(rect(8.0, y, 290.0, 26.0));
            row.layout.setFrame(rect(304.0, y - 1.0, 170.0, 28.0));
            row.remove.setFrame(rect(482.0, y - 1.0, 104.0, 28.0));
            row.remove.setTag(i as isize);
        }
    }
}
