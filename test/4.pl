#!/usr/local/bin/perl
# This is a test Perl script, for 
# test CodeLoc is working

=this
is a multi line comment
=cut

if ($#ARGV != 4) {
 print "usage: compem bg.rgb inbase outbase startNum stopNum\n";
 exit;
}

$bg = $ARGV[0];
$inbase = $ARGV[1];
$outbase = $ARGV[2];
$start = $ARGV[3];
$stop = $ARGV[4];

for ($i=$start; $i <= $stop; $i++) {

    $num = $i;
    if($i<10) { $num = "00$i"; }
    elsif($i<100) { $num = "0$i"; }

    $cmd = "over $bg $inbase.$num $outbase.$num 0 0";
    print $cmd."\n";
    if(system($cmd)) { print "over failed\n"; }
}
