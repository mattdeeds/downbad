# Markdown Preview Test Document

This file exercises a wide range of CommonMark features so you can verify the preview rendering in **downbad**.

---

## Inline Formatting

This is **bold text**, this is *italic text*, and this is ***bold italic***.
Here is `inline code`, and here is ~~strikethrough~~ text.

## Links and Images

- [Rust Programming Language](https://www.rust-lang.org)
- [egui on GitHub](https://github.com/emilk/egui)
- An auto-link: <https://commonmark.org>

## Headings

### Third Level

#### Fourth Level

##### Fifth Level

###### Sixth Level

## Block Quotes

> This is a block quote. It can span multiple lines
> and should render with a visible left border.
>
> > This is a nested block quote inside the first one.
> > It goes deeper.
>
> Back to the first level.

## Ordered Lists

1. First item
2. Second item
   1. Nested item A
   2. Nested item B
      1. Deeply nested
3. Third item

## Unordered Lists

- Apples
- Oranges
  - Blood orange
  - Navel orange
    - Cara cara
- Bananas
- Grapes

## Mixed Lists

1. Step one
   - Detail A
   - Detail B
2. Step two
   - Detail C
     1. Sub-detail
     2. Another sub-detail
3. Step three

## Task Lists

- [x] Implement editor
- [x] Add save functionality
- [x] Add exit confirmation
- [ ] Add markdown preview
- [ ] Add syntax highlighting

## Code Blocks

Inline: run `cargo build --release` to compile.

Fenced with language:

```rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    for i in 0..20 {
        println!("fib({}) = {}", i, fibonacci(i));
    }
}
```

```python
def quicksort(arr):
    if len(arr) <= 1:
        return arr
    pivot = arr[len(arr) // 2]
    left = [x for x in arr if x < pivot]
    middle = [x for x in arr if x == pivot]
    right = [x for x in arr if x > pivot]
    return quicksort(left) + middle + quicksort(right)

print(quicksort([3, 6, 8, 10, 1, 2, 1]))
```

```json
{
  "name": "downbad",
  "version": "0.1.0",
  "features": ["editor", "preview"],
  "dependencies": {
    "eframe": "0.33",
    "egui_commonmark": "0.22"
  }
}
```

Indented code block (4 spaces):

    #include <stdio.h>
    int main() {
        printf("Hello, world!\n");
        return 0;
    }

## Horizontal Rules

Above the rule.

---

Between rules.

***

Below the rules.

## Tables

| Feature        | Status      | Priority |
|----------------|-------------|----------|
| Raw editing    | Done        | High     |
| Save / Load    | Done        | High     |
| Exit confirm   | Done        | Medium   |
| Preview toggle | In progress | Medium   |
| Syntax hilite  | Planned     | Low      |
| Line wrapping  | Planned     | Low      |

Right-aligned and centered columns:

| Left | Center | Right |
|:-----|:------:|------:|
| 1    |   A    |  100  |
| 2    |   B    |  200  |
| 3    |   C    |  300  |
| 4    |   D    |  400  |

## Paragraphs and Line Breaks

This is the first paragraph. It has multiple sentences that should flow together
into a single block of text without any hard line breaks between them. The
renderer should wrap these lines naturally.

This is the second paragraph. There should be visible spacing between this
paragraph and the one above it.

This line has a hard break
right here (two trailing spaces).

## Escaping

These characters are escaped and should render literally:

\*not italic\* and \*\*not bold\*\*

\# Not a heading

\- Not a list item

## HTML Entities

Some common entities: &amp; &lt; &gt; &copy; &mdash; &ndash;

## Long Content for Scroll Testing

### Section 1: Lorem Ipsum

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor
incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis
nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.
Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu
fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in
culpa qui officia deserunt mollit anim id est laborum.

### Section 2: More Text

Curabitur pretium tincidunt lacus. Nulla gravida orci a odio. Nullam varius,
turpis et commodo pharetra, est eros bibendum elit, nec luctus magna felis
sollicitudin mauris. Integer in mauris eu nibh euismod gravida. Duis ac tellus
et risus vulputate vehicula. Donec lobortis risus a elit. Etiam tempor. Ut
ullamcorper, ligula ut dictum pharetra, nisi nunc fringilla magna, in commodo
elit erat nec turpis. Ut pharetra augue nec augue.

### Section 3: Technical Notes

When working with egui, keep in mind:

1. The immediate mode paradigm means the UI is rebuilt every frame
2. State must be stored externally (in your `App` struct)
3. Layout is handled automatically but can be customized
4. Input handling uses a polling model via `ctx.input()`
5. Custom painting is available through the `Painter` API

> **Note:** egui repaints only when there is user interaction by default.
> Call `ctx.request_repaint()` if you need continuous updates.

### Section 4: A Long Code Example

```rust
use eframe::egui;

struct MyApp {
    name: String,
    age: u32,
    items: Vec<String>,
    selected: Option<usize>,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            name: "World".to_owned(),
            age: 42,
            items: vec![
                "Alpha".to_owned(),
                "Beta".to_owned(),
                "Gamma".to_owned(),
                "Delta".to_owned(),
                "Epsilon".to_owned(),
            ],
            selected: None,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My Application");
            ui.horizontal(|ui| {
                ui.label("Your name: ");
                ui.text_edit_singleline(&mut self.name);
            });
            ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
            if ui.button("Click me").clicked() {
                println!("Hello, {}! You are {} years old.", self.name, self.age);
            }
            ui.separator();
            ui.label("Select an item:");
            for (i, item) in self.items.iter().enumerate() {
                let checked = self.selected == Some(i);
                if ui.selectable_label(checked, item).clicked() {
                    self.selected = Some(i);
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "My App",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::default()))),
    )
}
```

### Section 5: Deeply Nested Structure

- Level 1
  - Level 2
    - Level 3
      - Level 4: This is quite deep nesting and should still render correctly
        with proper indentation at each level.
  - Back to level 2
    - Another level 3
- Back to level 1

### Section 6: Multiple Block Quotes

> First quote block.

> Second quote block with **bold** and *italic* and `code`.

> Third quote block with a list inside:
> - Item one
> - Item two
> - Item three

---

*End of test document. If you can read this in preview mode, everything is working!*
