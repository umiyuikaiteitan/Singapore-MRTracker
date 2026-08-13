/* Page controls for the timetable.
 *
 * Everything here is an enhancement. Without it the timetable still
 * shows every departure and still prints; the buttons simply stay
 * hidden, because they start with a `needs-script` class. */
(function () {
  "use strict";

  /* The controls carry `needs-script`, which the stylesheet hides.
   * Removing the class is what reveals them; clearing an inline style
   * would leave the stylesheet rule in force. */
  var controls = document.querySelectorAll(".needs-script");
  for (var i = 0; i < controls.length; i++) {
    controls[i].classList.remove("needs-script");
  }

  var print = document.getElementById("print-page");
  if (print) {
    print.addEventListener("click", function () {
      window.print();
    });
  }

  var mono = document.getElementById("toggle-mono");
  if (mono) {
    mono.addEventListener("click", function () {
      var on = document.documentElement.classList.toggle("monochrome");
      mono.setAttribute("aria-pressed", on ? "true" : "false");
    });
    mono.setAttribute("aria-pressed", "false");
  }
})();
