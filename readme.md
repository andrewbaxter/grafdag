This is an interactive directed acyclic graph viewer, and editor. There are a lot of other tools out there like graphviz, mermaid, xdot, etc. plus interactive (in the sense that you need to manually layout everything) tools like excalidraw, visio, etc. But the created graphs are largely static, the layout algorithms are fairly poor, they fail with dense data, and they don't provide a lot of tools for exploration IMO. I wanted to dump data in and think about how to read it later.

This has:

- Automatic layout
- Layers
- Fully keyboard editable (if not it's a bug)
- Has some tools to clarify relations, like highlighting paths or connected features

Keyboard navigation is based around an "edge" selection model - if you select a start and end node you can move to siblings of the end node relative to the start node, or move forwards past the end, reverse directions.

Find the spec [here (schemask)](./schemask.json).
