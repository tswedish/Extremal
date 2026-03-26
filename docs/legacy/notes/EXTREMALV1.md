
# Extremal v1 Description

Extremal is a combinatorial graph search game. The goal of the game is to find the highest scoring graphs and post them on public leaderboards.

The graph scoring is defined in advance, per leaderboard server, in a deterministic way such that every isomorphism class is forms an ordered set, where more difficult to find graphs are ordered lower, so the game is a kind of golf. 

The proposed standard scoring system is based on clique counts of 2-colorings. There are known clique count lower bounds for 3-cliques via Goodman, and some bounds are known via proofs or bounds on Ramsey numbers via s,t graphs. What these two results have in common is counts of k-cliques on 2-colorings. Let's consider the 2 color k-clique histogram of a graph with "n" vertices. We count the number of cliques of each size from 3, 4, 5, ... up to we count zero cliques at some maximal k. Starting from k-1, we score a graph as the number of cliques for each color. Since there's a kind of symmetry in clique counts, we score k-1 as [Max(count_k-1_red, count_k-1_blue) , Min(count_k-1_red, count_k-1_blue)]. 

This scoring means that lower is better: we want to count as few cliques as possible for a given "n", starting from high k-cliques down to 3-cliques. We have bounds on Goodman, so we can define the "Goodman Gap" as the distance from this extremel value. This becomes the tie breaker, since eventually all things considered, k < k+1, so we want to minimize higher clique sizes to minimize our score. After the Goodman Gap we use the automorphism group size as an additional tie breaker, since finding symmetric low clique count graphs is interesting, in this case a higher |Aut| is better. IF we wanted to strictly keep golf mentality we will add 1/|Aut| to our score, which means we reduce score further with symmetries. Then finally the size of the hash of the canonically labeled graph is the final tie breaker. 

It's worth specifying that we want to use the graph6 standard to represent graphs, with a standard canonical labeling using a consensus method (if one exists). This graph6 representation could be encoded as b64 if we want to keep nice looking JSON. We will use blake3 to hash things in Extremal. We will index graphs using the hash of the graph6 representation. This will be used to deduplicate. We want to use language technologies that are well suited to agentic coding, so I'd like to keep things in rust, with typescript and robust web app frameworks for the web UIs. Pick from what works best in the prototypes.

The system is primarily divided into the role of leaderboard server and search worker ("server" and "worker"). 

## Leaderboard Server

The leaderboard server verifies graph submissions, and stores the top M ranking graphs. Graphs are scored on the server if the submission has a valid signature. We will use a standard format with graph representation and CID and such so we can easily copy/paste JSONs to represent graphs. A worker registers with a server using a signing key_id and public/private key pair. The `extremal` cli allows workers to generate keys, register them with servers, and sign and send JSON submissions. You can also query connected servers and request leaderboard data. The extremal API allows workers to be defined in a sort of plugin fashion (discussed more in the worker section). 

Note that we only want to allow graph submissions that are signed. We want to be able to see the list of graphs on the leaderboard for a given key_id. This detail page will show the submissions associated with that key_id and any other information about them they've updated using their key to register (or update info), using the extremal cli or API. Submission details will link these key_ids, and submissions will have a metadata section for workers to add things like commit hash and local worker IDs. This kind of information could be really useful for cross-team collaboration, and the current prototype design feels pretty good. 

A server runs a standard protocol and doesn't handle web UI at all. We could have a functioning Extremal network with servers connected to workers without any web UI. Besides receiving and validating signed graph submissions and adding them to the leaderboard if they rank, the server is also responsible for serving requests for leaderboard data. This data is used by workers, but also for interested browsers. Browsers may just want to get their graph scored by verified server, or see the graphs on the leaderboard. These users will likely use the web ui.

Servers also have their own pubic/private key pair, which they use to sign verification results. These results can be saved as "receipts" or used for servers to update their own leaderboards forming a kind of verification coherenece network for the top submission leaderboards. In practice, leaderboards may specialize in niche areas of graph search seeds, or be broad, actively updating their databases with other leaderboards.

The database will be suitable for cloud hosting, so likely using postgres, or some library that would make it easy to port to Cloud SQL or whatever cheap google hosting service we want to use. Note that I want this system to be hostable on Google cloud, with paths for hosting on other cloud platforms as well. Or self hosting, which is where most of the testing will be done. Because of this hosting consideration, I want to support easy horizontal scaling for handling validation requests.

### Web UI

The leaderboard web ui is updated when the leaderboard server admits new graph submissions. The graphs are represented as "gems" and their properties are mapped to their appearance on the site. The top "gems" are showcased and the UI should be exciting, highlighting recent events and the slowly evolving leaderboard rankings as workers compete for better scores. Leaderboards are indexed by "n" with servers deciding what "n" ranges they host. Browsers can check out different "n" leaderboards and watch as new submissions come in and make it onto the boads. Some kind of events stream (implemented in a robust way), could provide further visual interest to watch stats as graph submissions come in. I want this to be the place people can cheer on the workers, browsing the leaderboards and see what submissions are claimed by registered workers.

## Search Worker

The search workers are supported by the core API, with some default search workers included. Workers can run standalone in the same way the leaderboard server can. A separate web ui app can communicate with the workers, and provide a control dashboard.

The workers themselves can query servers for leaderboard data, and get things like the minimal score to get an admitted result.

The workers share much of the verification logic on the server, and so they should do their best to verify graphs, sign, and otherwise do the validation the server will do before submitting to the server. These things will be supported by the extremal core including the cli and API. Local users could interact via the extremal cli and see what scores graphs would get. 

The main interaction loop for users will be in creating workers. As such, there will be a few "built-in" workers like "tree", that will serve as examples for other plugins. "tree" should really try to be a plugin, and representative of a good implementation of a worker. In the development of Extremal, we will try to optimize and explore and develop the tree worker to help test and design the overall system. We hope others participate in this process, so Extremal will be open source, with github workflow CI, and potentially other community support. As such sharing things like git commits of submissions and such could be really fruitful. It's these commit hashes I expect to be interesting for web browers. These could even be used as input to other workers' developer workflows.

### Web UI

The web ui dashboard for the workers will be a web app that can connect to any number of workers running on the system, via hostname:port. The idea is the web ui will show a list of addresses (likely localhost:PORT), and you can connect to them and show a stream of data on the screen. I want to organize these streams as vertical columns, such that we can see worker local pool leaderboards and other statistics stream by next to each other like waterfalls, or Matrix code. Ideally I can render gems here (so in practice the dashboard UI and the leaderboard UI may have shared components or library?). There will be an info section in the vertical channel view for the connected workers' information, metadata, and so on. Unlike the prototype, the web app dashboard will not be for controlling the workers, but only connecting to and viewing their progress. 

# MVP System

The primary goal will be to produce the minimal viable system and test it for V1. This will include a central leaderboard server, hosted on the cloud. Remote workers can connect to it directly, or browser users can navigate to the web app. Developers can run servers and workers locally on their machines. Most development will be done locally on workers, which can be launched on local machines, and then submit any interesting graphs to the central server if any are found. I want to be able to launch 16 tree workers, watch them on the dashboard, and then see submissions flow into the public server. I want to be able to test all of this locally, and then migrate to the cloud.

# Design

When in doubt, refer to the RamseyNet prototype, as it sketches the basic workflow. This prototype was built iteratively, and conflicts with the above description, but should serve as a good guide for what I'm going for, so it can be a decent arbitor of intent, keeping in mind the more coherent and tighter implementation I have in mind and described above.