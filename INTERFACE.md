# Building what people look at

Read this before you build a page or a screen: the site, the Workshop, the app.

- Nothing stays blank and then fills in. Content that arrives after the page shows gets a
  placeholder in its exact place.
- Design the wait out first: show the last answer from a cache, and never wait on the server on
  purpose. A few seconds to load is a defect.
- Check the first load with the data source slowed down.
- A change to what the user sees or does is proven by driving the real built app through the
  flow, on throwaway `e2e-*` computers, never on the owner's own.
