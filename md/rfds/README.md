# Requests for Discussion (RFDs)

RFDs are a way of planning out larger changes. They aren't required but they can be useful.

The basic idea is that you open a PR adding an RFD based on the [RFD template](./TEMPLATE/README.md) into the `rfds` directory. Each RFD is itself a subdirectory like `rfds/my-rfd/README.md`. Be sure to add that to the SUMMARY.md file. RFDs can have subchapters or other accompanying material.

If the PR is accepted, the RFD will be merged in. At that point you open implementation PRs based on the RFD until it is completed. Each implementation PR should update the RFD to reflect its status.

Finally, you move it to the other section (the path stays the same).
