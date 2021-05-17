
# Advice for Contributing and using git

The main Theseus repository, [`theseus-os/Theseus`](https://github.com/theseus-os/Theseus) is what we call the *upstream*.
  To contribute, you should create your own fork of that repository through the GitHub website, and then check out your own fork.
  That way, your fork will be the `origin` remote by default, and then you can add the upstream as another remote by running:
  ```sh
  git remote add upstream https://github.com/theseus-os/Theseus
  ```

### Never push to the main branch
Currently, the main branch on the upstream `theseus-os/Theseus/theseus_main` is protected from a direct push. 
This is true even for GitHub users who are in the `theseus-os` organization and have write access to the Theseus repo.
The only way to contribute to it is by merging a pull request into the main branch, which only authorized users can do.
Instead, checkout your own fork as above, create a new branch with a descriptive name, e.g., `kevin/logging_typo`,
develop your feature on that branch, and then submit a pull request.
This is a standard Git workflow that allows people can review your code, check for pitfalls and compatibility problems,
and make comments and suggestions before the code makes its way into the main branch.
*You must do this for all changes, even tiny ones that may seem insignificant.*

### Submitting a pull request
To submit a pull request (PR), go to the GitHub page of your forked Theseus repo,
select the branch that you created from the drop down menu, and then click "New pull request".
By default, GitHub will create a new PR that wants to merge your branch into the upstream `theseus_main` branch,
which is usually what you want to do.
Now, give your PR a good title and description, scroll down to review the commits and files changed,
and if everything looks good, click "Create pull request" to notify the maintainers that you have contributions that they should review.

### Review your own work
Perform an initial review of your own code before submitting a pull request.
Kindly don't place the whole burden of fixing a bunch of tiny problems on others that must review your code too.
This includes building the documentation and reviewing it in HTML form in a browser (`make view-doc`)
to make sure everything is formatted correctly and that hyperlinks work correctly.

### Double-check commit contents
When making a commit, review your changes with `git status` and `git diff`, as well as on the GitHub comparison page, to ensure that you're not committing accidental modifications or editing files that you shouldn't be.
This makes the maintainers' lives a lot easier, meaning your PR is more likely to be accepted.

You don't need to worry about having too many small commits, as we will squash (combine) all of your PR's commits into a single large commit when merging it into the upstream main branch.
