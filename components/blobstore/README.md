*Blob store* is a set of components for the [flowd](https://github.com/ERnsTL/flowd/) system. They enable storage of binary blobs into a grouped/hierarchical state-keeping storage.

This is currently realized using a filesystem-backed storage. It uses a root data directory and a temporary file directory. To enable thread-safe access by multiple readers and writers, it uses the atomic file move technique from *maildir* file handling: Move and hardlink are atomic filesystem operations.

TODO IIP format and parameters for all components

TODO historical data writing and retrieval, option to keep n historic/previous values instead of overwriting the last value

TODO maybe store also

## Rationale

1. Why not just marshal/save the frame going into *blobstore-put* into its file?

  While that would preserve the datatype and other metadata present in the frame header, it makes reading/writing by other systems looking at the files more difficult (would have to implement framing format).

  TODO enable non-compatible format using an IIP switch, but does that really give something useful? Only some amount of self-description.
