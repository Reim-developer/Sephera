(* single line comment *)

(* multiple line comment, commenting out part of a program, and containing a
nested comment:
let f = function
  | 'A'..'Z' -> "Uppercase"
    (* Add other cases later... *)
*)

(* Real code, for test LOC is working. *)
let f = function
  | 'A'..'Z' -> "Uppercase" (* 
    Special comment again.
  *)

