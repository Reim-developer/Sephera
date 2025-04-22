// Test file for mixed code and comment lines
// Case 1: Code followed by comment start on same line
var x = 1 /*
*/

// Case 2: Code with comment in the middle
let y: number = 2; /* This is a comment */ let z: number = 3;

// Case 3: Code with comment at end that continues to next line
function test() {
    let a: number = 5; /* Comment starts here
    and continues on this line */
    return a;
}

// Case 4: Comment only line
/* This is just a comment */

// Case 5: Comment with nested comments
/* Outer comment
   /* Nested comment */
   let x_2: string = "";

// Case 6: Code with comment that has nested comments
let b = 10; /* Comment with /* nested */ let bx: boolean = false;

