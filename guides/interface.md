# Building what people look at

Read this before you build a page or a screen: the site, the Workshop, the app.

- Pages for people must be obvious to use and short on text: one place to look, no explanatory
  paragraphs, no assistant-sounding copy.
- The Workshop is where people meet the product, the owner first and any visitor after. It has to
  be convenient for both, and it has to make a visitor want the product. Features come before
  decisions, and a film before text.
- What a person came to look at is louder than the controls. Controls fit one line and stay few:
  what is always needed is visible, and the rest is one click away in a menu.
- No caption explains the page. A control's own content says what it is. What a mark means is in a
  tooltip of six words or fewer, shown on hover and on keyboard focus, and in its accessible name.
  A tooltip never holds a list.
- Show a relation, do not write it. What depends on what is drawn, by position, a line or a mark,
  never as a sentence of names.
- Compact: rows of one line, one spacing scale, columns on a grid.
- Components are one family, made from the page's own: one radius, one border, one way to show
  hover, focus, pressed and disabled. No native dropdown and no new colour.
- Every state is designed before it is built: empty, one result, hundreds, opened, filtered,
  loading, light and dark, wide and narrow.
- Before a page is designed, write its paths: each kind of person it should interest, what each
  wants to know, and each thing they look at or press to find it. Then fit the paths together, and
  decide what a person sees first. A design that serves every path looks obvious afterwards.
- A view that is new, or that changes how a page is used, is drawn first as a static page with
  real content, by an agent given these rules and nothing to build. The owner looks at the
  picture before anyone builds it.
- Nothing stays blank and then fills in. Content that arrives after the page shows gets a
  placeholder in its exact place.
- Design the wait out first: show the last answer from a cache, and never wait on the server on
  purpose. A few seconds to load is a defect.
- Check the first load with the data source slowed down.
- A page's scripted check (text cut or overlapping, sideways scroll, smallest text size, nothing
  moving the layout while it plays) runs in Playwright's Firefox, Chromium and WebKit. An agent
  looks at pictures in one browser only, and opens another browser's pictures only when the script
  fails there.
- A change to what the user sees or does is proven by driving the real built app through the
  flow, on throwaway `e2e-*` computers, never on the owner's own.
