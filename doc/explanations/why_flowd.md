## Why flowd exists

Wire up components (programs) written in different programming languages, using the best features and available libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames to produce a useful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Routing is available and a load-balancing component exists.

Use available off-the-shelf components where you can. Grow a collection of specialized components and *reuse* them for the next and next project of yours.

Thus, rather than rewriting code anew for each project, you become more and more efficient with regards to *human time* spent on development.

This is the basic idea of *Flow-based Programming* (FBP), as pioneered by [J. Paul Morrison](http://www.jpaulmorrison.com/fbp/).

The ```flowd``` (for *flow daemon*) is a *runtime environment* for the execution of FBP processing networks, to be defined by a programmer, which then constitutes an *application* or processing system of some kind.

The act of programming is thus shifted from entering strings and lines of tailor-made program source code to a more graphical and visual kind of programming with direct feedback of the changes just made, based on the combination and connection of re-usable *black boxes* (components) working together in a *visually drawable and mappable* processing network resp. application.

Such an FBP processing network is not limited to linear pipes, ETL steps, not even a directed acyclic graph (DAG) structure, RPC request-response, client-server, publish-subscribe, event streams etc. Instead, it is a versatile and generic superset allowing processing networks spanning multiple flowd instances and non-FBP processing systems and thus the creation of general processing systems and even interactive applications.

You can find out more about this paradigm on [J. Paul Morrison's website](http://www.jpaulmorrison.com/fbp/).

More, humans are terrible at writing, maintaining and understanding code, refer [a talk about this](https://www.youtube.com/watch?v=JhCl-GeT4jw). The solution proposed is not to fundamentally improve the way software is engineered, but to keep using conventional programming and just add another layer on top, namely to use AI to generate ever more piles of non-reusable custom application code. Unmentioned in the talk: For understanding and navigating it, one will need even more AI. The alternative, which FBP offers, is to go the other direction and keep applications on a humanly-understandable level by using these re-usable *black boxes* (components), which are individually all easily understandable, and connect them into compose software. FBP processing networks are humanly understandable also because they fit the steps, which a design team would use to model and break down the application's functionality, processing steps and data flows.